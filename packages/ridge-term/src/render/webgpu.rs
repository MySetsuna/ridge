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
//! `new(host)` returns `Err` when `GpuContext::get_or_init` fails (no
//! WebGPU adapter). `RenderHandle::newWithWebgpuFirst` then falls back
//! to `Canvas2dBackend` so the pane never crashes; the error string
//! is the only signal.

#![cfg(all(target_arch = "wasm32", feature = "webgpu"))]

use std::cell::RefCell;
use std::rc::Rc;

use super::glyph_atlas::{GlyphEntry, GlyphKey};
use super::gpu_context::GpuContext;
use super::surface_host::{ScissorRect, SurfaceHost};
use crate::render::procedural_box;
use crate::render::backend::{CursorDraw, FrameMetrics, RenderBackend, RowDraw, Theme};
use crate::term::cell::{scan_line_path, RenderPath};
use crate::term::attr_table::AttrTable;

/// High bit tag for grapheme-cluster glyph IDs so they cannot collide
/// with any Unicode codepoint (max 0x10FFFF).
const CLUSTER_TAG: u32 = 0x8000_0000;

/// CellInstance `is_color` sentinel for procedural rects (block-element /
/// box-drawing / shade chars). `cell.wgsl::fs_main` short-circuits this
/// value and returns the premultiplied fg directly, bypassing atlas
/// sampling — the procedural path's `atlas_uv = (0,0,0,0)` would otherwise
/// read the unreliable corner of layer 0 and pull coverage to ~0, making
/// the rect invisible. 0 = mono atlas glyph, 1 = color emoji, 2 = procedural.
const INSTANCE_MODE_PROCEDURAL: u32 = 2;

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
    is_color: u32,       // 68..72  — 0 = mono atlas glyph, 1 = color emoji bitmap, 2 = procedural rect (cell.wgsl short-circuits to premultiplied fg, skipping atlas sampling)
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
    /// §4b per-pane increment cache (2026-05-08). Number of valid
    /// CellInstance entries currently uploaded to `instance_buffer`
    /// from the last successful `end_frame`. `record_cached_only` uses
    /// this to re-issue the same instanced draw without retraversing
    /// the kernel grid. Reset to 0 by anything that would invalidate
    /// the cached instances:
    ///   - resize_surface (cell coords change)
    ///   - on_full_invalidate (renderer-side full-redraw signal)
    ///   - invalidate_atlas (UVs in cached instances point at evicted slots)
    ///   - cross-pane atlas-generation bump (same reason, detected in begin_frame)
    /// Updated by end_frame after a successful upload + record.
    cached_n_cells: u32,
    /// Snapshot of `ctx.atlas_eviction_count` at the last successful
    /// `end_frame`. If the count advanced, another pane evicted a layer
    /// that our cached instance buffer references — `record_cached_only`
    /// must fall back to full render to rebuild instances with correct
    /// atlas UV/layer data.
    cached_evictions_seen: u64,
    /// Distinct non-reserved atlas layers referenced by this pane's last
    /// successful `end_frame` instance upload. `pin_cached_layers` ORs
    /// these into the shared `frame_written` mask BEFORE any pane's
    /// full-render eviction runs this frame, so a cached-replay pane's
    /// already-recorded draw can't have its atlas slots evicted +
    /// overwritten mid-frame by another pane admitting new glyphs.
    cached_layers: Vec<u16>,
}

impl Drop for WebGpuPaneBackend {
    fn drop(&mut self) {
        // Drop bind_group, frame_uniform, and instance_buffer explicitly
        // (if wgpu needs it) or just let them drop naturally.
        // In wgpu-rs, buffers/bindgroups drop automatically on scope exit.
    }
}

impl WebGpuPaneBackend {
    /// Acquire (or reuse) the shared `GpuContext` + `SurfaceHost`, then
    /// allocate this pane's per-pane buffers + bind group. Async
    /// because the first call performs the full WebGPU adapter /
    /// device bootstrap; subsequent calls return the cached `Rc`
    /// immediately.
    ///
    /// Per-workspace SurfaceHost passed in by JS. Caller obtains the
    /// reference from a `SurfaceHostHandle` constructed for the
    /// pane's workspace tab — no thread-local lookup, multiple
    /// SurfaceHost instances coexist, one per workspace canvas.
    pub async fn new(host: Rc<RefCell<SurfaceHost>>) -> Result<Self, String> {
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
                tui_mode: false,
            },
            theme: Theme::default_dark(),
            // First frame must re-encode every row — viewport rect just
            // assigned by JS is fresh and the pane has never drawn.
            needs_initial_clear: true,
            cached_n_cells: 0,
            cached_evictions_seen: 0,
            cached_layers: Vec::new(),
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
        // P1.1's flag-driven version (returning `self.needs_initial_clear`)
        // assumed `desired_maximum_frame_latency: 1` makes
        // `get_current_texture` deterministically return frame N-1's
        // pixels and `LoadOp::Load` therefore reliably preserves prior
        // content. That assumption HOLDS on the e2e-shell release exe
        // (where the spec also passes), but DOES NOT hold inside
        // `pnpm tauri:dev:cdp` on Edge WebView2 148.0.3967.70 — there,
        // every cursor-blink `render()` call writes a "row 6 only"
        // instance buffer, presents over a swap-chain texture whose
        // prior pixels are silently dropped, and the user sees the
        // history rows blink in/out every 500 ms together with the
        // cursor.
        //
        // Forcing a full frame on every tick re-encodes every visible
        // row (~rows × cols cell instances on the idle blink) — wasteful
        // but visually correct regardless of swap-chain preservation
        // semantics. P1.1's CPU win was real but it traded correctness
        // for an env-specific optimisation; we'd rather pay the encode
        // cost than ship a fix that only renders right on the release
        // build.
        //
        // If a future Edge / WebView2 update makes LoadOp::Load reliable
        // again (e.g. via DXGI flip-discard with explicit retain), we
        // can re-introduce the flag-driven fast path behind a runtime
        // capability probe.
        //
        // TODO(①选区闪烁/首行选不中, 2026-06-18): 此恒-true 使活动 prompt 行被
        // PSReadLine 高频重画时每帧整屏 LoadOp::Clear → 选区闪烁 + 首行难选中
        // (首行=活动输入行)。修复需运行时取证后改为「初始化一次性能力探测」版:
        // LoadOp::Load 可靠 → 返回 self.needs_initial_clear(脏行快路径);否则保持
        // true。交接文档 docs/superpowers/specs/2026-06-18-selection-flash-firstline-handoff.md,
        // 追踪 docs/term-rebuild/TASKS.md §1.36。
        true
    }

    fn on_full_invalidate(&mut self) {
        // Renderer signalled a renderer-side full-redraw condition
        // (first frame, scroll offset change, selection toggle,
        // snapshot growth). Switch the next frame back to `LoadOp::Clear`
        // so the new row→content mapping doesn't paint over stale
        // background pixels left from the previous mapping.
        self.needs_initial_clear = true;
        // §4b: cached instances reflect the OLD scroll/selection state;
        // invalidate the cache so the next frame goes through full encode.
        self.cached_n_cells = 0;
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
            // §4b: cached instances were sized against the OLD viewport;
            // their cell_xy/cell_size values are stale. Drop the cache.
            self.cached_n_cells = 0;
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
        // §4b: cached instances reference now-invalid atlas slots.
        self.cached_n_cells = 0;
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
            // §4b: cached cell instances point at evicted atlas slots.
            self.cached_n_cells = 0;
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
        let bg_color = if self.metrics.tui_mode { self.theme.tui_bg } else { self.theme.bg };
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
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).round().max(1.0);
        let cell_h = (self.metrics.cell_h * self.metrics.dpr).round().max(1.0);
        let tui_mode = self.metrics.tui_mode;
        let theme = self.theme.clone();

        let mut row_bg_instances: Vec<CellInstance> = Vec::new();

        // Consume tracking: columns consumed by a preceding wide cell's
        // grid allocation have their bg covered — we skip them to
        // prevent an independent bg from visually cutting the emoji.
        let render_path = scan_line_path(row.cells, row.clusters);
        let mut consume_until: usize = 0;

        for (col, cell) in row.cells.iter().enumerate() {
            if cell.width == 0 {
                continue;
            }

            if col < consume_until {
                continue;
            }

            let (_attrs, fg, bg) =
                crate::render::backend::resolve_cell_colors(cell, attrs_table, &theme, tui_mode);

            let cell_span = cell.width.max(1) as usize;

            let pixel_x = (col as f32 * cell_w + 0.5).floor();
            let pixel_x_right = ((col + cell_span) as f32 * cell_w + 0.5).floor();
            let cell_w_px = pixel_x_right - pixel_x;

            let pixel_y = (row_idx as f32 * cell_h + 0.5).floor();
            let pixel_y_bot = ((row_idx + 1) as f32 * cell_h + 0.5).floor();
            let row_h_int = pixel_y_bot - pixel_y;

            row_bg_instances.push(CellInstance {
                cell_xy: [pixel_x, pixel_y],
                cell_size: [cell_w_px, row_h_int],
                atlas_uv: [0.0, 0.0, 0.0, 0.0],
                atlas_layer: 0,
                fg_rgba: rgba_u8_to_f32(fg),
                bg_rgba: rgba_u8_to_f32(bg),
                is_color: 0,
            });

            if render_path == RenderPath::Slow && cell_span > 1 {
                consume_until = col + cell_span;
            }
        }
        self.pending_instances.append(&mut row_bg_instances);
    }

    fn draw_row_texts(&mut self, row: &RowDraw<'_>, attrs_table: &AttrTable) {
        let row_idx = row.row_index;
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).round().max(1.0);
        let cell_h = (self.metrics.cell_h * self.metrics.dpr).round().max(1.0);
        let tui_mode = self.metrics.tui_mode;
        let theme = self.theme.clone();

        let mut row_glyph_instances: Vec<CellInstance> = Vec::new();

        let render_path = scan_line_path(row.cells, row.clusters);

        for (col, cell) in row.cells.iter().enumerate() {
            if cell.width == 0 {
                continue;
            }

            let attrs = attrs_table.get(cell.attr);
            let (_attrs, fg, _bg) =
                crate::render::backend::resolve_cell_colors(cell, attrs_table, &theme, tui_mode);

            let cell_span = cell.width.max(1) as usize;

            // Pixel-aligned positions — floor(pos + 0.5) prevents sub-pixel
            // seams between adjacent cells that would show as hairline gaps.
            let pixel_x = (col as f32 * cell_w + 0.5).floor();
            let pixel_y = (row_idx as f32 * cell_h + 0.5).floor();
            let pixel_y_bot = ((row_idx + 1) as f32 * cell_h + 0.5).floor();
            let row_h_int = pixel_y_bot - pixel_y;

            if cell.ch == ' ' && cell.attr == crate::term::attr_table::AttrId::DEFAULT {
                continue;
            }

            // ── Fast-path skip: for pure ASCII lines, no cluster lookup
            // is needed. This avoids the linear scan through `row.clusters`
            // and the per-cell char encoding for the common case of code
            // and log output, keeping the tight loop minimal.
            let cluster_text: Option<&str> = if render_path == RenderPath::Fast {
                None
            } else if !row.clusters.is_empty() {
                let target = col.min(u16::MAX as usize) as u16;
                row.clusters.iter().find(|c| c.col == target).map(|c| c.text.as_ref())
            } else {
                None
            };
            let mut ch_buf = [0u8; 4];
            let glyph_text: &str = match cluster_text {
                Some(text) => text,
                None => cell.ch.encode_utf8(&mut ch_buf),
            };

            let (font_family_hash, font_size_q) = {
                let ctx = self.ctx.borrow();
                let mut h = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&ctx.font_family, &mut h);
                (std::hash::Hasher::finish(&h), (ctx.font_size_px * 100.0).round() as u16)
            };
            let entry: Option<GlyphEntry> = {
                let mut ctx = self.ctx.borrow_mut();

                let mut style_flags = 0;
                if attrs.flags.contains(crate::term::attrs::Flags::BOLD) {
                    style_flags |= GlyphKey::STYLE_BOLD;
                }
                if attrs.flags.contains(crate::term::attrs::Flags::ITALIC) {
                    style_flags |= GlyphKey::STYLE_ITALIC;
                }

                let glyph_id = match cluster_text {
                    Some(text) => {
                        use std::hash::Hasher;
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        h.write(text.as_bytes());
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

                let lookup_hit = ctx.atlas.lookup(&key);
                let entry_opt: Option<GlyphEntry> = match lookup_hit {
                    Some(e) => {
                        if (e.layer as usize) < ctx.frame_written.len() {
                            ctx.frame_written[e.layer as usize] = true;
                        }
                        Some(e)
                    }
                    None => ctx
                        .rasterize_and_admit(
                            key,
                            glyph_text,
                            self.metrics.dpr,
                            style_flags,
                            &self.frame_pinned,
                        )
                        .ok(),
                };
                if let Some(e) = entry_opt {
                    self.frame_pinned[e.layer as usize] = true;
                    Some(e)
                } else {
                    None
                }
            };

            let is_color_flag: u32 =
                if entry.map(|e| e.is_color).unwrap_or(false) { 1 } else { 0 };

            // Procedural block/box-drawing chars
            let first_char = glyph_text.chars().next();
            let mut drawn_procedurally = false;

            if let Some(ch) = first_char {
                // Shade characters (U+2591..=U+2593) — full-cell quad
                // with the fg alpha scaled by 25 / 50 / 75 percent so
                // the rasterizer's antialiased glyph is replaced by a
                // resolution-independent shade that aligns to the cell
                // grid (btop / mc / shading-based gauges depend on
                // this). Handled here (not in `procedural_box`) so the
                // function signature stays opaque-rect-only.
                let shade_alpha: Option<f32> = match ch {
                    '\u{2591}' => Some(0.25), // ░ light
                    '\u{2592}' => Some(0.50), // ▒ medium
                    '\u{2593}' => Some(0.75), // ▓ dark
                    _ => None,
                };
                if let Some(alpha) = shade_alpha {
                    let mut fg_scaled = rgba_u8_to_f32(fg);
                    fg_scaled[3] *= alpha;
                    row_glyph_instances.push(CellInstance {
                        cell_xy: [pixel_x, pixel_y],
                        cell_size: [cell_span as f32 * cell_w, row_h_int],
                        atlas_uv: [0.0, 0.0, 0.0, 0.0],
                        atlas_layer: 0,
                        fg_rgba: fg_scaled,
                        bg_rgba: [0.0, 0.0, 0.0, 0.0],
                        is_color: INSTANCE_MODE_PROCEDURAL,
                    });
                    drawn_procedurally = true;
                } else if let Some(rects) =
                    procedural_box(ch, pixel_x, pixel_y, cell_span as f32 * cell_w, row_h_int)
                {
                    for r in rects {
                        // Pixel-snap to integer boundaries. When cell_w
                        // or cell_h is odd (which is common after the
                        // (px * dpr).round() step in line 520-521), the
                        // half/quarter expressions in procedural_box
                        // (cell_w * 0.5, cell_h * 0.125, …) land on
                        // half-pixel coordinates. The GPU then alpha-
                        // blends the rect's edge across two pixels,
                        // making the same character look thicker or
                        // thinner depending on whether its cell happens
                        // to sit on an even/odd row or column. Snapping
                        // each edge independently (NOT left + width)
                        // means ▀ and ▄ in the same column stay flush,
                        // and every ▀ on the screen renders at the
                        // identical integer height.
                        let r_x = r.x.round();
                        let r_y = r.y.round();
                        let r_right = (r.x + r.w).round();
                        let r_bot = (r.y + r.h).round();
                        let r_w = (r_right - r_x).max(1.0);
                        let r_h = (r_bot - r_y).max(1.0);
                        row_glyph_instances.push(CellInstance {
                            cell_xy: [r_x, r_y],
                            cell_size: [r_w, r_h],
                            atlas_uv: [0.0, 0.0, 0.0, 0.0],
                            atlas_layer: 0,
                            fg_rgba: rgba_u8_to_f32(fg),
                            bg_rgba: [0.0, 0.0, 0.0, 0.0], // Background already painted
                            is_color: INSTANCE_MODE_PROCEDURAL,
                        });
                    }
                    drawn_procedurally = true;
                }
            }

            if !drawn_procedurally {
                if let Some(e) = entry {
                    let natural_w = (e.px_w as f32).max(1.0);
                    row_glyph_instances.push(CellInstance {
                        cell_xy: [pixel_x, pixel_y],
                        cell_size: [natural_w, row_h_int],
                        atlas_uv: e.uv,
                        atlas_layer: e.layer as u32,
                        fg_rgba: rgba_u8_to_f32(fg),
                        bg_rgba: [0.0, 0.0, 0.0, 0.0],
                        is_color: is_color_flag,
                    });
                }
            }
        }
        self.pending_instances.append(&mut row_glyph_instances);
    }

    fn draw_cursor(&mut self, cursor: &CursorDraw, _attrs_table: &AttrTable) {
        use crate::render::backend::CursorStyle;

        let cell_w = (self.metrics.cell_w * self.metrics.dpr).round().max(1.0);
        let cell_h = (self.metrics.cell_h * self.metrics.dpr).round().max(1.0);
        let effective_col = cursor.col as f64;
        let pixel_x = (effective_col as f32 * cell_w + 0.5).floor();
        let cursor_span = cursor.width.max(1) as usize;

        // §B.9 — measure effective span via atlas lookup (px_w).
        // Falls back to cell_span on cache miss (next frame catches up).
        let effective_span = if cursor_span >= 2 {
            let (font_family_hash, font_size_q) = {
                let ctx = self.ctx.borrow();
                let mut h = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&ctx.font_family, &mut h);
                (std::hash::Hasher::finish(&h), (ctx.font_size_px * 100.0).round() as u16)
            };
            let glyph_id = match &cursor.cluster_text {
                Some(text) if !text.is_empty() => {
                    use std::hash::Hasher;
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    h.write(text.as_bytes());
                    let raw = std::hash::Hasher::finish(&h) as u32;
                    CLUSTER_TAG | (raw & !CLUSTER_TAG)
                }
                _ => cursor.ch as u32,
            };
            let key = GlyphKey {
                font_family_hash,
                font_size_q,
                glyph_id,
                style_flags: 0,
            };
            let entry = self.ctx.borrow_mut().atlas.lookup(&key);
            match entry {
                Some(e) => ((e.px_w as f32).max(1.0) / cell_w).ceil() as usize,
                None => cursor_span,
            }
        } else {
            cursor_span
        };

        let effective_span_f = effective_span as f64;
        let pixel_x_right =
            ((effective_col + effective_span_f) as f32 * cell_w + 0.5).floor();
        let span_w = pixel_x_right - pixel_x;

        let pixel_y = (cursor.row as f32 * cell_h + 0.5).floor();
        let pixel_y_bot = ((cursor.row + 1) as f32 * cell_h + 0.5).floor();
        let cell_h_int = pixel_y_bot - pixel_y;
        let bar_thickness = (2.0 * self.metrics.dpr).round().max(1.0);

        let (block_x, block_y, block_w, block_h) = match cursor.style {
            CursorStyle::Block => (pixel_x, pixel_y, span_w, cell_h_int),
            CursorStyle::Bar => (pixel_x, pixel_y, bar_thickness, cell_h_int),
            CursorStyle::Underline => (
                pixel_x,
                pixel_y + cell_h_int - bar_thickness,
                span_w,
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
            is_color: 0,
        });

        // Inverted glyph (Block only). Atlas-hit-only — we don't
        // rasterize-on-miss here. If the glyph isn't cached yet, the
        // next draw_row tick will populate it; cursor renders as a
        // solid block this frame, then inverts next frame.
        if matches!(cursor.style, CursorStyle::Block) && cursor.ch != ' ' {
            let (font_family_hash, font_size_q) = {
                let ctx = self.ctx.borrow();
                let mut h = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&ctx.font_family, &mut h);
                (std::hash::Hasher::finish(&h), (ctx.font_size_px * 100.0).round() as u16)
            };
            let glyph_id = match &cursor.cluster_text {
                Some(text) if !text.is_empty() => {
                    use std::hash::Hasher;
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    h.write(text.as_bytes());
                    let raw = std::hash::Hasher::finish(&h) as u32;
                    CLUSTER_TAG | (raw & !CLUSTER_TAG)
                }
                _ => cursor.ch as u32,
            };
            let key = GlyphKey {
                font_family_hash,
                font_size_q,
                glyph_id,
                style_flags: 0,
            };
            let entry: Option<GlyphEntry> = {
                let mut ctx = self.ctx.borrow_mut();
                ctx.atlas.lookup(&key)
            };
            if let Some(entry) = entry {
                let cursor_text_color = rgba_u8_to_f32(self.theme.cursor_text_color);
                let natural_w = (entry.px_w as f32).max(1.0);
                // §B.9 — natural size at effective column, no aspect-fit.
                let gx = (effective_col as f32 * cell_w + 0.5).floor();
                self.pending_instances.push(CellInstance {
                    cell_xy: [gx, pixel_y],
                    cell_size: [natural_w, cell_h_int],
                    atlas_uv: entry.uv,
                    atlas_layer: entry.layer as u32,
                    fg_rgba: cursor_text_color,
                    bg_rgba: cursor_color,
                    is_color: if entry.is_color { 1 } else { 0 },
                });
            }
        }
    }

    fn draw_selection_overlay(&mut self, rects: &[(usize, usize, usize)]) {
        if rects.is_empty() {
            return;
        }
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).round().max(1.0);
        let cell_h = (self.metrics.cell_h * self.metrics.dpr).round().max(1.0);
        let sel_color = rgba_u8_to_f32(self.theme.selection_bg);
        for &(row, col_start, col_end) in rects {
            if col_end <= col_start {
                continue;
            }
            let pixel_x = (col_start as f32) * cell_w;
            let pixel_x_right = (col_end as f32) * cell_w;
            let width = pixel_x_right - pixel_x;
            let pixel_y = (row as f32) * cell_h;
            let pixel_y_bot = (row + 1) as f32 * cell_h;
            let height = pixel_y_bot - pixel_y;
            self.pending_instances.push(CellInstance {
                cell_xy: [pixel_x, pixel_y],
                cell_size: [width, height],
                atlas_uv: [0.0, 0.0, 0.0, 0.0],
                atlas_layer: 0,
                fg_rgba: sel_color,
                bg_rgba: sel_color,
                is_color: 0,
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
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).round().max(1.0);
        let cell_h = (self.metrics.cell_h * self.metrics.dpr).round().max(1.0);
        let pixel_y = row as f32 * cell_h;
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
        let pixel_x_start = col as f32 * cell_w;
        let total_width = total_cells as f32 * cell_w;

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
        let mut cell_offset = 0usize;
        for (ch, width) in &char_widths {
            let key = GlyphKey {
                font_family_hash,
                font_size_q,
                glyph_id: *ch as u32,
                style_flags: 0,
            };
            let entry: Option<GlyphEntry> = {
                let mut ctx = self.ctx.borrow_mut();
                let glyph_str = ch.to_string();
                match ctx.atlas.lookup(&key) {
                    Some(e) => {
                        if (e.layer as usize) < ctx.frame_written.len() {
                            ctx.frame_written[e.layer as usize] = true;
                        }
                        Some(e)
                    }
                    None => ctx
                        .rasterize_and_admit(
                            key,
                            &glyph_str,
                            self.metrics.dpr,
                            0,
                            &self.frame_pinned,
                        )
                        .ok(),
                }
            };
            if let Some(e) = entry {
                if (e.layer as usize) < self.frame_pinned.len() {
                    self.frame_pinned[e.layer as usize] = true;
                }
                let pixel_x = (col + cell_offset) as f32 * cell_w;
                let natural_w = (e.px_w as f32).max(1.0);
                // §B.9 — natural size, no aspect-fit
                self.pending_instances.push(CellInstance {
                    cell_xy: [pixel_x, pixel_y],
                    cell_size: [natural_w, cell_h],
                    atlas_uv: e.uv,
                    atlas_layer: e.layer as u32,
                    fg_rgba: fg_color,
                    bg_rgba: [0.0, 0.0, 0.0, 0.0],
                    is_color: if e.is_color { 1 } else { 0 },
                });
            }
            cell_offset += *width as usize;
        }

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

    fn draw_hyperlink_underlines(&mut self, rects: &[(usize, usize, usize)]) {
        if rects.is_empty() {
            return;
        }
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).round().max(1.0);
        let cell_h = (self.metrics.cell_h * self.metrics.dpr).round().max(1.0);
        let thickness = (2.0 * self.metrics.dpr).round().max(1.0);
        let link_color = rgba_u8_to_f32(self.theme.hyperlink_color);
        for &(row, col_start, col_end) in rects {
            if col_end <= col_start {
                continue;
            }
            let pixel_x = (col_start as f32) * cell_w;
            let pixel_x_right = (col_end as f32) * cell_w;
            let width = pixel_x_right - pixel_x;
            let pixel_y_bot = (row + 1) as f32 * cell_h;
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
        let visible_count = overlay.items.len().min(overlay.max_visible_rows);
        if visible_count == 0 {
            return;
        }
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).round().max(1.0);
        let cell_h = (self.metrics.cell_h * self.metrics.dpr).round().max(1.0);

        const H_PAD_CELLS: f32 = 0.6;
        const V_PAD_CELLS: f32 = 0.35;
        let pad_w = H_PAD_CELLS * cell_w;
        let pad_h = V_PAD_CELLS * cell_h;

        const COL_CAP: usize = 80;
        let normalised: Vec<String> = overlay
            .items
            .iter()
            .take(visible_count)
            .map(|s| s.replace(['\r', '\n'], " ↵ "))
            .collect();
        let row_widths_cells: Vec<usize> = normalised
            .iter()
            .map(|s| {
                let mut w = 0usize;
                for c in s.chars() {
                    w += if (c as u32) < 0x80 { 1 } else { 2 };
                    if w >= COL_CAP {
                        break;
                    }
                }
                w.min(COL_CAP)
            })
            .collect();
        let panel_cells_w = row_widths_cells.iter().copied().max().unwrap_or(0).max(8);
        // §history-scroll — reserve room on the right for a scrollbar when the
        // full filtered list is longer than the visible window.
        let needs_scrollbar = overlay.total_items > visible_count;
        let sb_w = if needs_scrollbar { (cell_w * 0.30).clamp(4.0, 10.0) } else { 0.0 };
        let sb_gap = if needs_scrollbar { (cell_w * 0.18).max(2.0) } else { 0.0 };
        let panel_w = panel_cells_w as f32 * cell_w + 2.0 * pad_w + sb_w + sb_gap;
        let panel_h = visible_count as f32 * cell_h + 2.0 * pad_h;

        let panel_x = (overlay.anchor_col as f32 * cell_w).max(0.0);
        let panel_y_top = if overlay.place_above {
            ((overlay.anchor_row as f32) * cell_h - panel_h).max(0.0)
        } else {
            (overlay.anchor_row as f32 + 1.0) * cell_h
        };

        let inner_x = panel_x + pad_w;
        let inner_y = panel_y_top + pad_h;

        let bg = rgba_u8_to_f32(theme.bg);
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
        for (row_i, text) in normalised.iter().enumerate() {
            let row_y = inner_y + row_i as f32 * cell_h;
            let selected =
                overlay.selected_index >= 0 && row_i == overlay.selected_index as usize;
            let glyph_color = if selected { bg } else { fg };
            let mut cell_offset = 0usize;
            for ch in text.chars() {
                let ch_w_cells = if (ch as u32) < 0x80 { 1usize } else { 2usize };
                if cell_offset + ch_w_cells > panel_cells_w {
                    break;
                }
                let key = GlyphKey {
                    font_family_hash,
                    font_size_q,
                    glyph_id: ch as u32,
                    style_flags: 0,
                };
                let entry: Option<GlyphEntry> = {
                    let mut ctx = self.ctx.borrow_mut();
                    let glyph_str = ch.to_string();
                    match ctx.atlas.lookup(&key) {
                        Some(e) => {
                            if (e.layer as usize) < ctx.frame_written.len() {
                                ctx.frame_written[e.layer as usize] = true;
                            }
                            Some(e)
                        }
                        None => ctx
                            .rasterize_and_admit(
                                key,
                                &glyph_str,
                                self.metrics.dpr,
                                0,
                                &self.frame_pinned,
                            )
                            .ok(),
                    }
                };
                if let Some(e) = entry {
                    if (e.layer as usize) < self.frame_pinned.len() {
                        self.frame_pinned[e.layer as usize] = true;
                    }
                    let pixel_x = inner_x + (cell_offset as f32) * cell_w;
                    let natural_w = (e.px_w as f32).max(1.0);
                    // §B.9 — natural size, no aspect-fit
                    self.pending_instances.push(CellInstance {
                        cell_xy: [pixel_x, row_y],
                        cell_size: [natural_w, cell_h],
                        atlas_uv: e.uv,
                        atlas_layer: e.layer as u32,
                        fg_rgba: glyph_color,
                        bg_rgba: [0.0, 0.0, 0.0, 0.0],
                        is_color: if e.is_color { 1 } else { 0 },
                    });
                }
                cell_offset += ch_w_cells;
            }
        }

        // 4) 1-device-px border.
        let bw = 1.0_f32.max(self.metrics.dpr.round());
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
            // §4b: with 0 instances we have nothing to cache.
            self.cached_n_cells = 0;
            self.cached_layers.clear();
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
        // §4b: remember the just-uploaded instance count so a future
        // `record_cached_only` can re-issue this exact draw without
        // walking the kernel grid.
        self.cached_n_cells = n_cells;
        self.cached_evictions_seen = ctx.atlas_eviction_count;
        // §atlas-pin: record the distinct glyph layers this frame's
        // instances cite so `pin_cached_layers` can protect them next time
        // we replay via `record_cached_only`. Reserved layer 0
        // (backgrounds / clears / procedural rects) is never an eviction
        // candidate — skip it to keep the list tight.
        let mut layers: Vec<u16> = Vec::new();
        for inst in &self.pending_instances {
            let l = inst.atlas_layer;
            if l >= super::gpu_context::ATLAS_RESERVED_LAYERS {
                let lu = l as u16;
                if !layers.contains(&lu) {
                    layers.push(lu);
                }
            }
        }
        self.cached_layers = layers;
    }
}

#[cfg(target_arch = "wasm32")]
impl WebGpuPaneBackend {
    /// §4b per-pane increment cache (2026-05-08): re-record this pane's
    /// PREVIOUSLY-uploaded instance buffer into the host's current
    /// frame WITHOUT retraversing the kernel grid, generating new
    /// CellInstances, or re-uploading them. The vertex buffer in
    /// `instance_buffer` already holds the last successful frame's
    /// data; we just need to ask the host to record another draw call
    /// against it inside the pane's scissor.
    ///
    /// Returns `false` (caller must fall back to a full `render` /
    /// `end_frame` cycle) when:
    ///   - `cached_n_cells == 0` (no prior frame, OR cache was
    ///     invalidated by `on_full_invalidate` / `resize_surface` /
    ///     `invalidate_atlas` / atlas-generation bump);
    ///   - viewport is empty (pane scissor collapsed to 0×0 — drawing
    ///     would be a no-op anyway);
    ///   - the host is in a `needs_initial_clear` state for this pane
    ///     (paint correctness requires re-encoding from scratch).
    ///
    /// Returns `true` after successfully recording the draw — the next
    /// `SurfaceHost::end_frame` will include this pane's pixels.
    ///
    /// Used by JS `manager.ts::startRafLoop` for visible host-mode
    /// panes that pre-pass marked NOT dirty: the swap-chain `LoadOp::
    /// Clear` would otherwise wipe their region (forcing a re-encode
    /// even for unchanged content), and this method is the cheap path
    /// that paints the cached pixels back without a kernel grid sweep.
    /// On a typing-into-one-pane workload with N other static panes,
    /// the per-tick CPU cost of those N panes drops from O(rows × cols)
    /// per pane to one GPU draw call per pane.
    pub fn record_cached_only(&mut self) -> bool {
        if self.cached_n_cells == 0
            || self.viewport.is_empty()
            || self.needs_initial_clear
        {
            return false;
        }
        // Defensive: an atlas-generation bump must invalidate cached
        // UVs. Catch the case where invalidation happened between
        // begin_frame's check and this call (e.g., another pane in the
        // same RAF tick triggered atlas rebuild).
        let ctx = self.ctx.borrow();
        if ctx.atlas_generation != self.atlas_generation_seen {
            self.cached_n_cells = 0;
            return false;
        }
        // Cross-pane atlas eviction guard: if another pane evicted a
        // layer since our last `end_frame`, our cached instance buffer
        // may reference stale atlas data. Fall back to full render so
        // `draw_row_texts` re-rasterizes and re-uploads with correct
        // layer assignments.
        if ctx.atlas_eviction_count != self.cached_evictions_seen {
            self.cached_n_cells = 0;
            return false;
        }
        drop(ctx);

        // Re-upload the frame uniform — cheap (16 bytes) and guards
        // against any out-of-band viewport change since last frame.
        let viewport_uniform: [f32; 4] =
            [self.viewport.w as f32, self.viewport.h as f32, 0.0, 0.0];
        let n_cells = self.cached_n_cells;
        let viewport = self.viewport;
        let bind_group = &self.bind_group;
        let instance_buffer = &self.instance_buffer;
        let ctx = self.ctx.borrow();
        ctx.queue.write_buffer(
            &self.frame_uniform,
            0,
            bytemuck::bytes_of(&viewport_uniform),
        );
        self.host
            .borrow_mut()
            .record_pane(viewport, &ctx.cell_pipeline, |pass| {
                pass.set_bind_group(0, bind_group, &[]);
                pass.set_vertex_buffer(0, instance_buffer.slice(..));
                pass.draw(0..4, 0..n_cells);
            });
        true
    }

    /// §atlas-pin: re-pin this pane's cached glyph layers into the shared
    /// per-frame `frame_written` mask. Called by the host loop right after
    /// the host frame opens (mask just reset) and before any pane's full
    /// render — so eviction in `rasterize_and_admit` won't reclaim a layer
    /// that this pane's upcoming `record_cached_only` replay still samples.
    /// No-op when the cache is empty/invalid (then `record_cached_only`
    /// itself falls back to full render and re-marks layers as it admits).
    pub fn pin_cached_layers(&mut self) {
        if self.cached_n_cells == 0 || self.cached_layers.is_empty() {
            return;
        }
        let mut ctx = self.ctx.borrow_mut();
        for &l in &self.cached_layers {
            let idx = l as usize;
            if idx < ctx.frame_written.len() {
                ctx.frame_written[idx] = true;
            }
        }
    }
}
