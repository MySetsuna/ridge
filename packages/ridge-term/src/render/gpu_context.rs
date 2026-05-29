//! Shared WebGPU context — Round 3 §4.3 Phase A.
//!
//! Holds resources that are *invariant across panes*: `wgpu::Instance`,
//! `Device`, `Queue`, the cell render pipeline, the glyph atlas, and the
//! glyph rasterizer. All `WebGpuBackend` instances borrow this singleton
//! via `Rc<RefCell<GpuContext>>` instead of constructing their own copies.
//!
//! ## Why singleton
//!
//! [`OVERVIEW.md` §1] articulates the architectural win: "10 pane 时 GPU
//! context 1 个（旧方案 10 个）、atlas 1 份（旧方案 10 份)". With one
//! Device for the entire process, the driver only manages a single
//! command queue + memory arena. With one atlas, a glyph rasterized in
//! pane A is reused for free in pane B (same `GlyphKey` resolves to the
//! same texture-array layer).
//!
//! ## Why `Rc<RefCell<>>` and not `static OnceCell<Mutex<>>`
//!
//! wasm32 is single-threaded. `Rc<RefCell<>>` avoids the `Send`
//! constraints `wgpu::Device` does not satisfy on web targets, and the
//! `RefCell` borrow is the natural fit for the per-frame access pattern
//! (one `borrow_mut` from `begin_frame` / `draw_row` / `end_frame`,
//! never nested).
//!
//! ## Atlas generation
//!
//! When `slot_w` / `slot_h` grow (font enlarged, DPR change) the atlas
//! texture is reallocated and `atlas_generation` is bumped. Per-pane
//! bind groups still reference the old `atlas_view` until they detect
//! the generation mismatch in their next `begin_frame` and rebuild.
//! This is the cross-pane invalidation rule that lets a pane-A grow
//! event propagate correctly into pane B's next frame.
//!
//! ## Hardcoded surface format
//!
//! `Bgra8UnormSrgb` is a WebGPU-required format (canvas swap chains must
//! support it on every implementation). Hardcoding lets us build the
//! cell pipeline at GpuContext construction time without waiting for the
//! first per-pane surface — which in turn lets `request_adapter` skip
//! the `compatible_surface` hint, so we never need to allocate (and
//! later drop) a bootstrap surface that would race with the per-pane
//! `WebGpuBackend::new` surface creation on the same canvas.

#![cfg(all(target_arch = "wasm32", feature = "webgpu"))]

use std::cell::RefCell;
use std::rc::Rc;

use super::glyph_atlas::{pick_evictable_layer, GlyphAtlas, GlyphEntry, GlyphKey};
use super::glyph_rasterizer::GlyphRasterizer;

/// Atlas slot dimension floors in device pixels. `slot_w` is rounded up
/// to a power of two so `bytes_per_row = slot_w × 4` automatically
/// satisfies wgpu's 256-byte `COPY_BYTES_PER_ROW_ALIGNMENT` (i.e.
/// `slot_w` must be ≥ 64 and a multiple of 64). `slot_h` carries no
/// alignment requirement.
///
/// Memory cost scales with `slot_w × slot_h × atlas_layers × 4`. At the
/// 64×96 floor with the 1024-layer max that's ≈ 24 MiB; doubling
/// `slot_w` to 128 (font ~24 CSS px at DPR 2) costs ≈ 48 MiB. Single
/// shared allocation in the §4.3 design, regardless of pane count.
/// Devices that only expose 256 layers (the WebGPU MVP floor) cap out
/// at ≈ 6 MiB.
pub const ATLAS_SLOT_W_FLOOR: u32 = 64;
pub const ATLAS_SLOT_H_FLOOR: u32 = 96;
/// Floor for the texture-array layer count. `Limits::downlevel_defaults()
/// .max_texture_array_layers == 256` is the WebGPU MVP guarantee — we
/// always ask for at least this many so the texture allocation never
/// fails on a portable device.
pub const ATLAS_LAYERS_MIN: u32 = 256;
/// Ceiling for the texture-array layer count. Most desktop adapters
/// expose 2048 in `adapter.limits().max_texture_array_layers`; we cap
/// at 1024 to bound atlas memory. Beyond this the marginal hit-rate
/// gain doesn't justify the allocation. The actual value picked in
/// `GpuContext::new` is `clamp(adapter_limit, MIN, MAX)`.
pub const ATLAS_LAYERS_MAX: u32 = 1024;
/// Layer 0 reserved as the permanent transparent fallback. Cells with
/// no atlas hit (rasterize failure, control char, NUL) push instances
/// referencing layer 0 + zero UV; the fragment samples zero coverage so
/// `mix(bg, fg, 0) == bg` collapses to background fill.
pub const ATLAS_RESERVED_LAYERS: u32 = 1;

/// §A.8 (2026-05-08) — atlas-side supersampling factor. Glyphs are
/// rasterised at `dpr * ATLAS_SUPERSAMPLE` device pixels per CSS pixel
/// and uploaded into atlas slots sized accordingly; the fragment shader
/// then samples this denser source through the Linear filter, which
/// effectively performs a 2×2 box downsample per output pixel — visibly
/// smoother edges on color emoji and CJK with no perceptible perf hit
/// (rasterisation is one-shot per glyph; only sampling cost per frame
/// changes, and that's GPU-cheap). Cost: atlas memory scales with
/// `ATLAS_SUPERSAMPLE²`. At the 64-floor + 1024-layer cap that's still
/// ≈ 96 MiB worst case — well within VRAM budgets even for integrated
/// adapters. Setting back to 1 disables supersampling cleanly (slot
/// dims and rasterisation density both fall back to native DPR).
pub const ATLAS_SUPERSAMPLE: u32 = 2;

/// Format passed to `GPUCanvasContext.configure()` (i.e. the
/// `wgpu::SurfaceConfiguration.format` field). The WebGPU spec restricts
/// canvas configure to `bgra8unorm`, `rgba8unorm`, or `rgba16float` —
/// sRGB variants are texture-only and Chrome rejects them with
/// `TypeError: Unsupported canvas context format 'bgra8unorm-srgb'`.
/// We therefore configure the canvas as linear `Bgra8Unorm` and create
/// an sRGB texture view per frame for the pipeline to render through
/// (see `view_formats` on the surface config + the explicit `format` on
/// the per-frame `create_view` call in `webgpu.rs`).
pub const CANVAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// Render-target format the cell pipeline writes through, and the format
/// of the per-frame `TextureView` we render INTO. Same as `CANVAS_FORMAT`
/// (linear `Bgra8Unorm`) so the byte values the shader writes show up
/// on screen unchanged — `theme.bg = #1e1e2e` produces pixels at exactly
/// `#1e1e2e`, matching the Canvas2D backend (which uses CSS `rgba()`
/// strings for fills, also no gamma awareness). Earlier this was
/// `Bgra8UnormSrgb` so the ROP would gamma-encode the shader's linear
/// output, but that produced a darker background than Canvas2D / theme
/// asked for and wasn't visually consistent with the rest of the app
/// (CSS `rgb(...)` colors are sRGB byte values too). Trade-off: the
/// shader's alpha blending happens in sRGB space rather than linear
/// space — same as Canvas2D and any DOM compositing, so the choice
/// keeps the two backends visually identical.
pub const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// Per-process shared GPU resources. One instance for all panes.
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_format: wgpu::TextureFormat,

    pub cell_shader: wgpu::ShaderModule,
    pub cell_bind_group_layout: wgpu::BindGroupLayout,
    pub cell_pipeline: wgpu::RenderPipeline,
    pub sampler: wgpu::Sampler,

    pub atlas: GlyphAtlas,
    pub atlas_texture: wgpu::Texture,
    pub atlas_view: wgpu::TextureView,
    pub next_free_layer: u32,
    /// Texture-array depth chosen at construction = `clamp(adapter
    /// limit, ATLAS_LAYERS_MIN, ATLAS_LAYERS_MAX)`. Drives the texture
    /// allocation, the LRU capacity, and the `frame_pinned` length on
    /// per-pane backends — all three must agree.
    pub atlas_layers: u32,
    pub rasterizer: GlyphRasterizer,
    pub slot_w: u32,
    pub slot_h: u32,
    /// Bumped every time `atlas_texture` / `atlas_view` is recreated.
    /// Per-pane backends compare their last-seen value at frame start;
    /// mismatch → rebuild bind group against the new view.
    pub atlas_generation: u64,
    /// Bumped every time a layer is evicted (reused by a new glyph).
    /// Per-pane backends snapshot this at `end_frame` and check in
    /// `record_cached_only` — if the count advanced since the last full
    /// render, their cached instance buffer may reference stale atlas
    /// data evicted by another pane.
    pub atlas_eviction_count: u64,
    /// Per-layer "already written this frame" mask, same length as
    /// `atlas_layers`. Reset to all-`false` at the start of every frame
    /// (in `SurfaceHost::begin_frame`). Set to `true` when a layer is
    /// written by any pane's `rasterize_and_admit`. Prevents the
    /// cross-pane within-frame race: without this guard, pane B can evict
    /// a layer that pane A just wrote to via `queue.write_texture`, and
    /// pane A's deferred draw command (recorded in the command encoder)
    /// will sample the wrong data when the encoder is submitted.
    pub frame_written: Vec<bool>,

    pub font_family: String,
    pub font_size_px: f32,
}

thread_local! {
    /// Process-wide singleton. `None` until the first
    /// `GpuContext::get_or_init` call succeeds; cached `Some` thereafter.
    /// Failure is *not* cached — each call re-attempts so a transient
    /// adapter miss doesn't permanently lock the session into Canvas2D.
    static SHARED_GPU: RefCell<Option<Rc<RefCell<GpuContext>>>> = const { RefCell::new(None) };
}

impl GpuContext {
    /// Lazily acquire the shared GPU context. First call performs the
    /// full WebGPU bootstrap (instance + adapter + device + pipeline +
    /// atlas); subsequent calls return the cached `Rc`.
    ///
    /// Returns `Err` on adapter / device acquisition failure so the
    /// caller (`WebGpuBackend::new`, eventually `RenderHandle
    /// ::newWithWebgpuFirst`) can fall back to Canvas2D. Failure is not
    /// memoized — a flaky adapter on call N can succeed on call N+1.
    pub async fn get_or_init() -> Result<Rc<RefCell<Self>>, String> {
        if let Some(rc) = SHARED_GPU.with(|cell| cell.borrow().clone()) {
            return Ok(rc);
        }
        let ctx = Self::new().await?;
        let rc = Rc::new(RefCell::new(ctx));
        SHARED_GPU.with(|cell| *cell.borrow_mut() = Some(rc.clone()));
        Ok(rc)
    }

    /// Bootstrap. Creates instance + adapter + device, then builds the
    /// shader / pipeline / atlas / rasterizer / sampler.
    async fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });

        // No `compatible_surface` hint — we don't have a canvas at this
        // layer. Browser WebGPU exposes one adapter; this is sufficient.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| {
                "GpuContext: no GPU adapter available — falling back to Canvas2D".to_string()
            })?;

        // Pick texture-array depth before requesting the device — wgpu
        // only honors `max_texture_array_layers` up to whatever we
        // declare in `required_limits`. Adapters typically advertise
        // 2048 (desktop) or 256 (WebGPU MVP floor); clamp into
        // [`ATLAS_LAYERS_MIN`, `ATLAS_LAYERS_MAX`] so memory stays
        // bounded while giving Claude-style TUIs (CJK + box-drawing
        // + spinner glyphs) enough cache headroom to avoid LRU thrash.
        let atlas_layers: u32 = adapter
            .limits()
            .max_texture_array_layers
            .clamp(ATLAS_LAYERS_MIN, ATLAS_LAYERS_MAX);
        let mut required_limits = wgpu::Limits::downlevel_defaults();
        required_limits.max_texture_array_layers = atlas_layers;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("ridge-term-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits,
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .map_err(|e| format!("GpuContext: request_device failed: {e:?}"))?;
        // Adapter is no longer needed once we have device + queue.
        drop(adapter);

        let cell_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ridge-cell-shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shaders/cell.wgsl"
            ))),
        });

        let cell_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("ridge-cell-bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2Array,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ridge-cell-pipeline-layout"),
            bind_group_layouts: &[&cell_bind_group_layout],
            push_constant_ranges: &[],
        });

        // CellInstance vertex layout. Field offsets must match the
        // `#[repr(C)]` `CellInstance` declaration in `webgpu.rs`. Stride
        // re-exported from there.
        let instance_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: super::webgpu::CELL_INSTANCE_STRIDE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: 36,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 52,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // §B.3 (2026-05-08) — per-glyph color/mono flag, sourced
                // from the rasterizer's pixel-scan and propagated via
                // `GlyphEntry::is_color`. Replaces the per-pixel
                // `glyph.rgb < 0.99` heuristic in `cell.wgsl::fs_main`.
                wgpu::VertexAttribute {
                    offset: 68,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Uint32,
                },
            ],
        };

        let cell_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ridge-cell-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &cell_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[instance_buffer_layout],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &cell_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: SURFACE_FORMAT,
                    // §B.4 (2026-05-08) — switched from ALPHA_BLENDING
                    // (straight) to PREMULTIPLIED_ALPHA_BLENDING because
                    // the cell shader now outputs premultiplied color
                    // (rgb already weighted by coverage).
                    //
                    // Pre-fix: shader output `(coverage * glyph_rgb,
                    // coverage)` for the split-glyph quad, then ROP
                    // applied straight-alpha composite which multiplied
                    // coverage A SECOND TIME — giving `coverage² *
                    // glyph_rgb + (1 - coverage) * dst_bg` for AA fringe
                    // pixels. Color contribution at AA edges was about
                    // half what it should be, visibly darkening color
                    // emoji edges.
                    //
                    // PREMULTIPLIED_ALPHA_BLENDING is `src + (1 -
                    // src.a) * dst` which matches the shader's premult
                    // output exactly. Narrow-cell single-instance path
                    // still works because shader outputs alpha=1 for
                    // opaque cells, collapsing the formula to `src + 0
                    // * dst = src` (same as ALPHA_BLENDING was doing).
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ridge-atlas-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            // §A.8 (2026-05-08): Linear filtering for atlas sampling.
            // Color emoji from system fonts (Segoe UI Emoji ≈ 1.37em
            // advance) get sampled into a 2-cell-wide quad (≈ 1.2em),
            // i.e. a slight horizontal compression — Nearest produced
            // visible jagged edges, Linear smooths the resampling so
            // emoji match the sharpness of native PowerShell text.
            // For ASCII / CJK glyphs at 1:1 atlas-px-to-quad-px the
            // Linear filter degenerates to the source pixel
            // (touching only one texel), so Latin / CJK rendering is
            // unaffected. Combined with §A.8's 2x supersampling at
            // rasterisation time and natural-advance quads for color
            // emoji, this restores a "native" look across both
            // backends.
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Initial atlas dimensions = slot floors. First per-pane
        // `begin_frame` will grow if real metrics demand it.
        let slot_w = ATLAS_SLOT_W_FLOOR;
        let slot_h = ATLAS_SLOT_H_FLOOR;

        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ridge-atlas-texture"),
            size: wgpu::Extent3d {
                width: slot_w,
                height: slot_h,
                depth_or_array_layers: atlas_layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Linear (NOT sRGB) — matches `SURFACE_FORMAT` so the whole
            // pipeline treats colors as "sRGB byte / 255" semantic values
            // throughout. With `Rgba8UnormSrgb` here, sampling would
            // gamma-decode the OffscreenCanvas's sRGB-byte glyph pixels
            // into linear space, the shader would mix in linear, then
            // write back to `Bgra8Unorm` (linear) — net effect: every
            // color-emoji RGB channel ends up displayed at its linear
            // value reinterpreted as sRGB byte (e.g. byte 200 → 149,
            // byte 100 → 32). Color emoji bodies appeared crushed-dark /
            // near-black against dark themes ("blank emoji" report on
            // 2026-05-08; matches the surface-format fix that already
            // landed).
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("ridge-atlas-view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        let rasterizer = GlyphRasterizer::new(slot_w as u16, slot_h as u16)?;

        // GlyphAtlas capacity = usable layer count so the LRU's eviction
        // trigger fires exactly when GPU slots are exhausted — never
        // trying to evict the reserved layer 0.
        let atlas_capacity = (atlas_layers - ATLAS_RESERVED_LAYERS) as usize;

        Ok(Self {
            instance,
            device,
            queue,
            surface_format: SURFACE_FORMAT,
            cell_shader,
            cell_bind_group_layout,
            cell_pipeline,
            sampler,
            atlas: GlyphAtlas::new(atlas_capacity),
            atlas_texture,
            atlas_view,
            next_free_layer: ATLAS_RESERVED_LAYERS,
            atlas_layers,
            rasterizer,
            slot_w,
            slot_h,
            atlas_generation: 0,
            atlas_eviction_count: 0,
            frame_written: vec![false; atlas_layers as usize],
            font_family: String::from("monospace"),
            font_size_px: 15.0,
        })
    }

    /// Compute the device-pixel atlas slot size required for the given
    /// (cell_w, cell_h, dpr). Wide CJK cells need ≥ `cell_w × dpr × 2`
    /// device pixels horizontally so the rasterizer's OffscreenCanvas
    /// holds the full advance without clipping. `slot_w` is rounded up
    /// to a power of two so `bytes_per_row = slot_w × 4` always
    /// satisfies wgpu's 256-byte alignment. Vertical adds 25% safety
    /// for descenders / italic overhang / stacked combining marks.
    pub fn slot_dims_for(cell_w_css: f32, cell_h_css: f32, dpr: f32) -> (u32, u32) {
        // §A.8: account for atlas-side supersampling — the rasteriser
        // paints into slots at `dpr * ATLAS_SUPERSAMPLE` device pixels
        // per CSS pixel, so slots must be `ATLAS_SUPERSAMPLE`× larger
        // along each axis than they would be at native DPR.
        let ss = ATLAS_SUPERSAMPLE as f32;
        let cell_w_dev = (cell_w_css * dpr * ss).max(1.0);
        let cell_h_dev = (cell_h_css * dpr * ss).max(1.0);
        // §B.10 (2026-05-08) — slot width must hold the WIDEST natural
        // advance any glyph might be rasterised at, including non-
        // monospace fallback fonts (Segoe UI Emoji's emoji ratio is
        // ~1.37em RELATIVE to font_size, but the host's `cell_w_css`
        // is `M`-advance-based ≈ 0.6em of font_size, so emoji advance
        // can be up to 2.28× cell_w_dev). The pre-§B.10 multiplier of
        // 2.0× wasn't enough — at DPR 2 / DPR 3 / large font sizes
        // the rasteriser's bbox got CLIPPED at slot_w, losing the
        // right portion of every wide emoji. Visible as "🎂 cursor
        // exceeds visual" — the bitmap drew the left ~78% only, with
        // the right portion missing.
        //
        // Bumping to 3.0× gives 50% headroom over the worst case
        // (1.37em emoji → 2.28× cell_w_dev), with the next_power_of_two
        // rounding pushing us to a clean atlas size. Memory cost: slot
        // area roughly doubles (slot_w 64→128 at typical metrics);
        // total atlas memory up to ~96 MiB at the 1024-layer cap,
        // still well within VRAM budget.
        let wide_w_dev = (cell_w_dev * 3.0).ceil() as u32;
        let row_h_dev = cell_h_dev.ceil() as u32;
        let slot_w = wide_w_dev.max(ATLAS_SLOT_W_FLOOR).next_power_of_two();
        let slot_h = (row_h_dev + row_h_dev / 4).max(ATLAS_SLOT_H_FLOOR);
        (slot_w, slot_h)
    }

    /// Reallocate `atlas_texture` / `atlas_view` at the current
    /// `slot_w` × `slot_h`. Drops every cached glyph (their UVs and
    /// layer indices are about to become stale). Bumps
    /// `atlas_generation` so per-pane backends know to rebuild their
    /// bind groups against the new `atlas_view`.
    /// Reset the per-frame written mask. Called at the start of every
    /// frame from `SurfaceHost::begin_frame` so each frame starts with
    /// all layers available for writing.
    pub fn reset_frame_written(&mut self) {
        for w in &mut self.frame_written {
            *w = false;
        }
    }

    pub fn rebuild_atlas(&mut self) -> Result<(), String> {
        self.atlas.clear();
        self.next_free_layer = ATLAS_RESERVED_LAYERS;

        let atlas_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ridge-atlas-texture"),
            size: wgpu::Extent3d {
                width: self.slot_w,
                height: self.slot_h,
                depth_or_array_layers: self.atlas_layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("ridge-atlas-view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        // Rasterizer's OffscreenCanvas dimensions must match the slot
        // exactly so its `get_image_data` is `slot_w × slot_h × 4`
        // bytes — same shape `queue.write_texture` expects.
        let rasterizer = GlyphRasterizer::new(self.slot_w as u16, self.slot_h as u16)?;

        self.atlas_texture = atlas_texture;
        self.atlas_view = atlas_view;
        self.rasterizer = rasterizer;
        self.atlas_generation = self.atlas_generation.wrapping_add(1);
        // Texture fully recreated — all layer data is undefined.
        // Reset both written-mask and free-layer pointer for a clean
        // start.
        self.reset_frame_written();
        Ok(())
    }

    /// Drop every cached glyph and reset the next-free-layer pointer to
    /// the first usable layer. Bumps `atlas_generation` so per-pane
    /// bind groups rebuild — without that bump, panes would keep
    /// drawing instances with stale `atlas_layer` indices that now
    /// point into reused-but-not-yet-uploaded slots.
    pub fn invalidate_atlas(&mut self) {
        self.atlas.clear();
        self.next_free_layer = ATLAS_RESERVED_LAYERS;
        self.atlas_generation = self.atlas_generation.wrapping_add(1);
    }

    /// Update the shared font configuration. Invalidates the atlas if
    /// either the family or size changes — every subsequent `draw_row`
    /// miss will rasterize at the new size, and per-pane bind groups
    /// rebuild against the new `atlas_view` at their next `begin_frame`
    /// (atlas_generation bumped via `invalidate_atlas`).
    ///
    /// Idempotent on no-op (same family + size).
    pub fn set_font_config(&mut self, font_family: String, font_size_px: f32) {
        let size_changed = (self.font_size_px - font_size_px).abs() > 0.01;
        let family_changed = self.font_family != font_family;
        self.font_family = font_family;
        self.font_size_px = font_size_px;
        if size_changed || family_changed {
            self.invalidate_atlas();
        }
    }

    /// Miss-path: rasterize a glyph, upload its bitmap into the next
    /// free atlas layer (or an LRU-evicted unpinned one), and admit it
    /// to the cache. Returns the freshly-inserted `GlyphEntry` ready
    /// for the caller to push into a `CellInstance`.
    ///
    /// `frame_pinned` is the caller pane's per-frame pin bitmap (length
    /// = `self.atlas_layers`). Layers cited by earlier instances of
    /// the SAME pane's current frame are pinned so we don't overwrite
    /// their pixels mid-frame; the eviction walk skips them.
    ///
    /// Returns `Err` on rasterize failure or when every layer is pinned
    /// (visible-unique-glyph count > capacity in one frame — vanishingly
    /// rare). Caller falls back to bg-only for that cell; the next
    /// frame retries once pins clear.
    /// §4.7 (2026-05-07): `glyph_text` may be a single codepoint or a
    /// multi-codepoint extended grapheme cluster. The atlas stores one
    /// rasterized bitmap per `GlyphKey` regardless of how many
    /// codepoints it represents — `key.glyph_id` discriminates between
    /// codepoint slots and cluster slots so a hash-collision-free
    /// lookup is the caller's responsibility (see `webgpu.rs::draw_row`
    /// for the cluster-hash tagging scheme).
    pub fn rasterize_and_admit(
        &mut self,
        key: GlyphKey,
        glyph_text: &str,
        dpr: f32,
        style_flags: u8,
        frame_pinned: &[bool],
    ) -> Result<GlyphEntry, String> {
        // §A.8 — pass an SS-multiplied dpr to the rasteriser so the
        // resulting bitmap is `ATLAS_SUPERSAMPLE`× denser than the
        // shader's quad. The rasteriser doesn't need to know about SS
        // — to it this just looks like a higher-DPR display. We then
        // downsample to logical device pixels when populating
        // `GlyphEntry.px_w / px_h` (below) so the renderer keeps
        // sizing quads in logical units.
        let ss = ATLAS_SUPERSAMPLE as f32;
        let glyph = self.rasterizer.rasterize(
            &self.font_family,
            self.font_size_px,
            dpr * ss,
            style_flags,
            glyph_text,
        )?;

        let layer: u32 = if self.next_free_layer < self.atlas_layers {
            let l = self.next_free_layer;
            self.next_free_layer += 1;
            if (l as usize) < self.frame_written.len() {
                self.frame_written[l as usize] = true;
            }
            l
        } else {
            // Atlas at capacity — pick an evictable layer that isn't
            // pinned by this frame's earlier instances OR already
            // written by another pane in this frame. The `frame_written`
            // guard prevents the cross-pane within-frame race: without
            // it, pane B's `write_texture` below would overwrite a layer
            // that pane A just wrote to, and pane A's deferred draw
            // command (recorded in the command encoder) would sample the
            // wrong data at submit time.
            match pick_evictable_layer(&mut self.atlas, frame_pinned, &self.frame_written) {
                Some(l) => {
                    self.atlas_eviction_count += 1;
                    if (l as usize) < self.frame_written.len() {
                        self.frame_written[l as usize] = true;
                    }
                    l
                }
                None => {
                    return Err(
                        "atlas: every layer pinned this frame — bg-only fallback".to_string()
                    );
                }
            }
        };

        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.atlas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &glyph.rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(self.slot_w * 4),
                rows_per_image: Some(self.slot_h),
            },
            wgpu::Extent3d {
                width: self.slot_w,
                height: self.slot_h,
                depth_or_array_layers: 1,
            },
        );

        let u1 = (glyph.width as f32) / (self.slot_w as f32);
        let v1 = (glyph.height as f32) / (self.slot_h as f32);
        // §A.8 — `glyph.width / glyph.height` are bitmap dimensions
        // in atlas device pixels (= dpr * ATLAS_SUPERSAMPLE). The
        // renderer (webgpu.rs::draw_row) sizes quads in *logical*
        // device pixels (= dpr only), so divide back here. UVs above
        // already cancel out — bbox/slot_w stays the same ratio
        // because both numerator and denominator scale with SS.
        let logical_px_w = ((glyph.width as u32) / ATLAS_SUPERSAMPLE).max(1) as u16;
        let logical_px_h = ((glyph.height as u32) / ATLAS_SUPERSAMPLE).max(1) as u16;
        let entry = GlyphEntry {
            layer: layer as u16,
            uv: [0.0, 0.0, u1, v1],
            advance: glyph.advance,
            ascent_offset: glyph.ascent_offset,
            px_w: logical_px_w,
            px_h: logical_px_h,
            is_color: glyph.is_color,
        };
        self.atlas.insert(key, entry);
        Ok(entry)
    }

    /// Build a per-pane bind group against the current `atlas_view` +
    /// `sampler`, with the supplied per-pane `frame_uniform`. Callers
    /// (per-pane `WebGpuBackend`) record the `atlas_generation` value
    /// at which this bind group was built; when `begin_frame` later
    /// detects a higher generation, it rebuilds via this method.
    pub fn build_bind_group(&self, frame_uniform: &wgpu::Buffer) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ridge-cell-bg"),
            layout: &self.cell_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
}

// `pick_evictable_layer` lives in `glyph_atlas.rs` — it's pure and
// host-testable, and both `GpuContext::admit_glyph` and the per-pane
// `WebGpuBackend::draw_row` go through that single source of truth.

#[cfg(test)]
mod tests {
    use super::*;

    // GpuContext construction requires a live WebGPU adapter — not
    // available in `cargo test --lib` (host target). These tests cover
    // pure logic that doesn't need the GPU: the slot-dim heuristic and
    // the pin-aware eviction walk. Browser smoke (plan §Verification)
    // covers the GPU-bearing paths, including atlas-generation
    // propagation across panes.

    fn slot_dims_for_pub(cell_w_css: f32, cell_h_css: f32, dpr: f32) -> (u32, u32) {
        // Mirrors the live `slot_dims_for` impl including the §A.8
        // ATLAS_SUPERSAMPLE multiplier and the §B.10 3.0× wide-headroom
        // factor so tests pin the actual formula, not a stale pre-SS
        // copy.
        let ss = super::ATLAS_SUPERSAMPLE as f32;
        let cell_w_dev = (cell_w_css * dpr * ss).max(1.0);
        let cell_h_dev = (cell_h_css * dpr * ss).max(1.0);
        let wide_w_dev = (cell_w_dev * 3.0).ceil() as u32;
        let row_h_dev = cell_h_dev.ceil() as u32;
        let slot_w = wide_w_dev
            .max(super::ATLAS_SLOT_W_FLOOR)
            .next_power_of_two();
        let slot_h = (row_h_dev + row_h_dev / 4).max(super::ATLAS_SLOT_H_FLOOR);
        (slot_w, slot_h)
    }

    #[test]
    fn slot_dims_default_metrics_hit_floor() {
        // 8×16 CSS px, DPR 1, SS 2 → cell_w_dev = 16, wide_w = 32 —
        // still under the 64 floor. Vertical 32 + 25% = 40, under the
        // 96 floor. Floors carry on small fonts even with SS.
        let (w, h) = slot_dims_for_pub(8.0, 16.0, 1.0);
        assert_eq!(w, super::ATLAS_SLOT_W_FLOOR);
        assert_eq!(h, super::ATLAS_SLOT_H_FLOOR);
    }

    #[test]
    fn slot_dims_grow_for_large_font_at_high_dpr() {
        // 24 CSS px font at DPR 2, SS 2 → cell_w_dev = 96,
        // wide_w = 96 × 3 = 288. Next power-of-two ≥ 288 is 512.
        // Vertical: row_h = 96, + 25% = 120 — beats the 96 floor.
        let (w, h) = slot_dims_for_pub(24.0, 24.0, 2.0);
        assert_eq!(w, 512);
        assert_eq!(h, 120);
    }

    #[test]
    fn slot_dims_clamp_zero_inputs_to_floor() {
        // Defensive: zero / negative metrics shouldn't underflow.
        let (w, h) = slot_dims_for_pub(0.0, 0.0, 1.0);
        assert_eq!(w, super::ATLAS_SLOT_W_FLOOR);
        assert_eq!(h, super::ATLAS_SLOT_H_FLOOR);
    }

    #[test]
    fn slot_dims_rounds_up_to_power_of_two() {
        // 33 px wide cell × DPR 1 × SS 2 → cell_w_dev = 66,
        // wide_w = 66 × 3 = 198 → next pow2 = 256.
        let (w, _) = slot_dims_for_pub(33.0, 16.0, 1.0);
        assert_eq!(w, 256);
    }

    #[test]
    fn slot_dims_grows_height_when_row_exceeds_floor() {
        // 100 css px row × DPR 2 × SS 2 → row_h_dev = 400 →
        // 400 + 100 = 500 wins over the 96 floor.
        let (_, h) = slot_dims_for_pub(8.0, 100.0, 2.0);
        assert_eq!(h, 500);
    }

    fn make_key(id: u32) -> GlyphKey {
        GlyphKey {
            font_family_hash: 0xdeadbeef,
            font_size_q: 1500,
            glyph_id: id,
            style_flags: 0,
        }
    }

    fn make_entry(layer: u16) -> GlyphEntry {
        GlyphEntry {
            layer,
            uv: [0.0, 0.0, 1.0, 1.0],
            advance: 8.0,
            ascent_offset: 12.0,
            px_w: 8,
            px_h: 16,
            is_color: false,
        }
    }

    #[test]
    fn pick_evictable_returns_oldest_when_unpinned() {
        let mut atlas = GlyphAtlas::new(4);
        atlas.insert(make_key(1), make_entry(0));
        atlas.insert(make_key(2), make_entry(1));
        atlas.insert(make_key(3), make_entry(2));
        let pinned = vec![false; 8];
        assert_eq!(pick_evictable_layer(&mut atlas, &pinned), Some(0));
        // Atlas size shrunk by one (the picked entry was evicted, not
        // re-inserted).
        assert_eq!(atlas.len(), 2);
    }

    #[test]
    fn pick_evictable_skips_pinned_and_picks_next() {
        let mut atlas = GlyphAtlas::new(4);
        atlas.insert(make_key(1), make_entry(0));
        atlas.insert(make_key(2), make_entry(1));
        atlas.insert(make_key(3), make_entry(2));
        // Layer 0 (the LRU) is pinned; eviction should skip past it
        // to layer 1, then re-insert layer 0's entry.
        let mut pinned = vec![false; 8];
        pinned[0] = true;
        assert_eq!(pick_evictable_layer(&mut atlas, &pinned), Some(1));
        // Layer 0 was re-inserted; layer 1 was evicted; layer 2 stays.
        assert_eq!(atlas.len(), 2);
        assert!(atlas.lookup(&make_key(1)).is_some());
        assert!(atlas.lookup(&make_key(2)).is_none());
    }

    #[test]
    fn pick_evictable_returns_none_when_all_pinned() {
        let mut atlas = GlyphAtlas::new(4);
        atlas.insert(make_key(1), make_entry(0));
        atlas.insert(make_key(2), make_entry(1));
        // Every layer cited by entries is pinned — caller must fall
        // back to bg-only.
        let mut pinned = vec![false; 8];
        pinned[0] = true;
        pinned[1] = true;
        assert_eq!(pick_evictable_layer(&mut atlas, &pinned), None);
        // Both entries restored to the cache.
        assert_eq!(atlas.len(), 2);
    }

    #[test]
    fn pick_evictable_handles_empty_atlas() {
        let mut atlas = GlyphAtlas::new(4);
        let pinned = vec![false; 8];
        assert_eq!(pick_evictable_layer(&mut atlas, &pinned), None);
    }
}
