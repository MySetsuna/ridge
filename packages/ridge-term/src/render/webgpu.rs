//! WebGPU rendering backend — Round 3 §4.1.
//!
//! ## Status
//!
//! **Slice 1 of N: GPU pipeline reaches canvas.**
//!
//! - `new(canvas).await` → adapter / device / surface acquired, surface
//!   configured with a 1×1 placeholder size (caller must call
//!   `resize_surface` before the first paint).
//! - `resize_surface(w, h, dpr)` → reconfigures the swap chain to
//!   match the canvas backing buffer.
//! - `clear()` + `end_frame()` → submits a single RenderPass that fills
//!   the surface with `theme.bg`. No glyph / cursor / overlay drawing
//!   yet — those trait methods are no-ops for now.
//!
//! What this proves: WebGPU adapter request succeeds, surface is
//! reachable, and a per-frame RenderPass can paint to the canvas.
//! The renderer's normal dirty-row machinery still drives it — when no
//! rows are dirty, it skips the whole `clear` + `draw_*` + `end_frame`
//! cycle, so the no-op draw methods never see a populated frame.
//!
//! ## Next slices (deferred)
//!
//! - §4.1.b: glyph rasterizer (cosmic-text or fontdue), texture-array
//!   atlas upload, and `draw_row` instance buffer.
//! - §4.1.c: cursor + selection-overlay + hyperlink-underline pipeline
//!   passes (full-quad shader + scissor).
//! - §4.3: shared surface across panes (one canvas, scissor per pane).
//!
//! ## Adapter-miss policy
//!
//! `new` returns `Err` on adapter / device acquisition failure so the
//! caller (the JS-side `RenderHandle` constructor, eventually) can
//! fall back to `Canvas2dBackend`. The error string is the only signal
//! — we don't try to recover internally because Canvas2D is a fully-
//! capable correctness oracle and there's no reason to crash the pane.

#![cfg(all(target_arch = "wasm32", feature = "webgpu"))]
#![allow(dead_code)] // round-3 §4.1 first slice; draw methods are still no-ops.

use super::glyph_atlas::{pick_evictable_layer, GlyphAtlas, GlyphEntry, GlyphKey};
use super::glyph_rasterizer::GlyphRasterizer;
use crate::render::backend::{CursorDraw, FrameMetrics, RenderBackend, RowDraw, Theme};
use crate::term::attr_table::AttrTable;
use web_sys::HtmlCanvasElement;

/// Convert an `[u8; 4]` RGBA color into a wgpu linear-color triple.
/// wgpu expects sRGB framebuffer stores, but the Color value passed to
/// `LoadOp::Clear` is in *linear* color space (the surface's sRGB
/// view applies the OETF on store). For simplicity we pass the raw
/// 0..1 normalized bytes — visually close enough for the bg-only
/// slice; round 4.1.c can revisit for color-management correctness.
fn rgba_to_wgpu_color(rgba: [u8; 4]) -> wgpu::Color {
    wgpu::Color {
        r: (rgba[0] as f64) / 255.0,
        g: (rgba[1] as f64) / 255.0,
        b: (rgba[2] as f64) / 255.0,
        a: (rgba[3] as f64) / 255.0,
    }
}

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

/// WebGPU backend — round 3 §4.1.c body. Holds the device, queue,
/// surface, the cell render pipeline + bind-group layout, and the
/// allocated GPU resources (texture-array atlas, sampler, frame
/// uniform buffer, per-instance buffer, bind group). draw_row body
/// remains a no-op for this slice — that wiring lands in §4.1.c.next.
pub struct WebGpuBackend {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// The cell vertex+fragment shader compiled from `shaders/cell.wgsl`.
    /// Cached so future hot-reload could swap it without rebuilding the
    /// pipeline_layout / bind_group_layout.
    cell_shader: wgpu::ShaderModule,
    /// Bind-group layout matching the WGSL `@group(0)` triple
    /// (uniform buffer + texture_2d_array + sampler). Used both at
    /// pipeline-create time and at every-frame bind-group instantiation.
    cell_bind_group_layout: wgpu::BindGroupLayout,
    /// Render pipeline driving cell.wgsl. One pipeline handles all
    /// cells (bg-only + glyph-bearing + bold/italic) — style differences
    /// are font-CSS at rasterization time, not pipeline switches.
    cell_pipeline: wgpu::RenderPipeline,
    /// Glyph atlas texture array. Each layer is one slot for a single
    /// rasterized glyph (white-on-transparent RGBA8). Layer count is
    /// the GlyphAtlas LRU capacity — eviction frees a layer for reuse.
    atlas_texture: wgpu::Texture,
    /// View over `atlas_texture` as a 2D array (matches the WGSL
    /// `texture_2d_array<f32>` binding).
    atlas_view: wgpu::TextureView,
    /// Linear-filtering sampler for atlas reads. Shared across all
    /// pane backends since terminal cell sampling is uniform.
    sampler: wgpu::Sampler,
    /// 16-byte uniform buffer holding `FrameUniform { viewport, _pad }`.
    /// queue.write_buffer'd at the start of every frame.
    frame_uniform: wgpu::Buffer,
    /// Per-cell instance buffer. Initial capacity = `INITIAL_INSTANCE_CAPACITY`
    /// cells; future iterations grow on demand via `device.create_buffer`
    /// when a frame's cell count exceeds capacity.
    instance_buffer: wgpu::Buffer,
    instance_capacity: u32,
    /// Bind group instance against `cell_bind_group_layout`. Reusable
    /// across frames — only the buffer + texture contents change, not
    /// the binding shape.
    bind_group: wgpu::BindGroup,
    /// OffscreenCanvas-based glyph rasterizer. Sized to match
    /// (`slot_w`, `slot_h`) — see [`Self::slot_dims_for`] — so its
    /// output bitmap fits exactly into one atlas-texture layer with
    /// no clipping. Recreated together with `atlas_texture` whenever
    /// metrics push the required slot beyond the current allocation.
    rasterizer: GlyphRasterizer,
    /// Per-frame CellInstance accumulator. `begin_frame` clears it,
    /// `draw_row` pushes one entry per non-continuation cell, the
    /// future `end_frame` body uploads it via `queue.write_buffer`
    /// and submits a single `draw(0..4, 0..len)` call.
    ///
    /// Sized lazily to match the largest frame seen so far — Vec
    /// keeps capacity across `clear()` so steady-state allocation
    /// settles after a few frames.
    pending_instances: Vec<CellInstance>,
    /// Next free atlas-texture-array layer. Incremented on each new
    /// glyph admitted to the atlas; once it reaches `self.atlas_layers`
    /// new misses pick an evictable layer via `pick_evictable_layer`
    /// (LRU among unpinned layers) and reuse it.
    next_free_layer: u32,
    /// Actual texture-array depth chosen at backend construction —
    /// `clamp(adapter.limits().max_texture_array_layers,
    /// ATLAS_LAYERS_MIN, ATLAS_LAYERS_MAX)`. Stored on the struct
    /// because it drives the texture allocation, the LRU capacity, and
    /// the `frame_pinned` length, all of which need to agree.
    atlas_layers: u32,
    /// Per-layer pin flag, reset to all-`false` every `begin_frame`.
    /// A layer is pinned the moment any cell in this frame's
    /// `pending_instances` references it (atlas-hit OR fresh insert),
    /// so the eviction path can skip layers whose pixels would be
    /// overwritten before the GPU samples them.
    ///
    /// Guards against the in-frame race where `evict_oldest` returns a
    /// layer already cited by an earlier instance in the same frame:
    /// `queue.write_texture` would overwrite that layer's bitmap before
    /// `end_frame` submits the render pass, so the earlier cell would
    /// sample the new glyph and visibly morph from frame to frame.
    /// Length always equals `atlas_layers`; indexed by layer id.
    frame_pinned: Vec<bool>,
    /// CSS font-family used for glyph rasterization. Stored so
    /// draw_row can pass it to `rasterizer.rasterize()` without
    /// rethreading per call. Defaults to "monospace" until a future
    /// `set_font_config` method (analogous to Canvas2dBackend's
    /// set_font) wires the user's terminal font setting.
    font_family: String,
    /// Font size in CSS pixels (post-DPR-divide; rasterizer multiplies
    /// internally). Defaults to 15 to match Canvas2dBackend.
    font_size_px: f32,
    atlas: GlyphAtlas,
    /// Current atlas slot width in device pixels (per layer). Computed
    /// from the active `FrameMetrics` in `begin_frame` via
    /// [`Self::slot_dims_for`]; bumped + atlas rebuilt when the
    /// metrics demand a larger slot than the existing allocation.
    /// Rounded up to a power of two so `bytes_per_row` stays aligned
    /// to wgpu's `COPY_BYTES_PER_ROW_ALIGNMENT` without padding.
    slot_w: u32,
    slot_h: u32,
    metrics: FrameMetrics,
    theme: Theme,
}

/// Atlas slot dimension floors in device pixels. Actual per-instance
/// `slot_w` / `slot_h` live on `WebGpuBackend` and are computed from
/// the current `FrameMetrics` via [`WebGpuBackend::slot_dims_for`] —
/// growing past these floors when `cell_w × dpr × 2` (wide CJK) or
/// `cell_h × dpr` exceed them, so a "中" glyph never gets clipped at
/// the slot boundary on HiDPI / large-font setups (2026-05-06 fix:
/// previously a hard `64` const truncated the right half of CJK
/// glyphs whenever `font_size_px × dpr > 64` — visible as "右半缺失").
///
/// `slot_w` is rounded up to a power of two so `bytes_per_row =
/// slot_w × 4` automatically satisfies wgpu's 256-byte
/// COPY_BYTES_PER_ROW_ALIGNMENT (slot_w must be ≥ 64 and a multiple of
/// 64). `slot_h` carries no such constraint.
///
/// Memory cost scales with `slot_w × slot_h × atlas_layers × 4`. At
/// the default 64×96 floor with 1024 layers that's ≈ 24 MiB; doubling
/// slot_w to 128 (font ~24 CSS px @ DPR 2) costs ≈ 48 MiB. Per-pane
/// today; collapses to one shared atlas in §4.3. Devices that only
/// expose 256 layers (the WebGPU MVP floor) cap out at ≈ 6 MiB.
const ATLAS_SLOT_W_FLOOR: u32 = 64;
const ATLAS_SLOT_H_FLOOR: u32 = 96;
/// Floor for the texture-array layer count. `Limits::downlevel_defaults()
/// .max_texture_array_layers == 256` is the WebGPU MVP guarantee — we
/// always ask for at least this many so the texture allocation never
/// fails on a portable device.
const ATLAS_LAYERS_MIN: u32 = 256;
/// Ceiling for the texture-array layer count. Most desktop adapters
/// expose 2048 in `adapter.limits().max_texture_array_layers`; we cap at
/// 1024 to bound atlas memory at `slot_w × slot_h × 1024 × 4` bytes
/// (default slot 64×96 ≈ 24 MiB, the wide-CJK 128×96 case ≈ 48 MiB).
/// Beyond this the marginal hit-rate gain doesn't justify the per-pane
/// allocation. The actual layer count picked at construction is
/// `clamp(adapter_limit, MIN, MAX)` and stored on the backend so the
/// runtime keeps working on hardware that exposes anything in between.
const ATLAS_LAYERS_MAX: u32 = 1024;
/// Layer 0 is reserved as a permanent transparent fallback (§4.5.d).
/// Cells with no atlas hit (rasterizer failure / ascii-NUL / control char)
/// push CellInstance with `atlas_layer = 0` + `atlas_uv = (0,0,0,0)` so the
/// fragment samples a zero-coverage texel and `mix(bg, fg, 0) == bg`.
/// Without this reservation the first rasterized glyph would land on
/// layer 0 and the fallback would sample its top-left pixel — for emoji
/// or filled-cell glyphs that's a visible color leak.
const ATLAS_RESERVED_LAYERS: u32 = 1;
/// Initial per-frame cell instance buffer capacity. Realistic terminal
/// sessions have a few thousand cells; 1024 covers small panes and the
/// buffer grows on demand for larger ones.
const INITIAL_INSTANCE_CAPACITY: u32 = 1024;

/// CPU-side instance struct matching the WGSL `InstanceIn` layout.
/// `#[repr(C)]` makes the field order load-bearing — must mirror the
/// `attributes: &[VertexAttribute { offset, ... }]` array passed to
/// `RenderPipelineDescriptor::vertex.buffers`.
///
/// Pod + Zeroable allow `bytemuck::cast_slice(&[CellInstance])` to
/// return `&[u8]` without unsafe transmutes (§4.5.c). Layout: 6 fields,
/// all f32 / u32 / [f32; N] arrays, 4-byte aligned, 68 bytes total — no
/// implicit padding so `Pod` is sound.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[allow(dead_code)] // Populated by §4.1.c draw_row body in a future iteration.
struct CellInstance {
    cell_xy: [f32; 2],   // 0..8
    cell_size: [f32; 2], // 8..16
    atlas_uv: [f32; 4],  // 16..32
    atlas_layer: u32,    // 32..36
    fg_rgba: [f32; 4],   // 36..52
    bg_rgba: [f32; 4],   // 52..68
}

pub(super) const CELL_INSTANCE_STRIDE: u64 = std::mem::size_of::<CellInstance>() as u64;

impl WebGpuBackend {
    /// Acquire a WebGPU adapter and device for `canvas`. Async because
    /// `request_adapter` and `request_device` return Promises that
    /// resolve asynchronously in the browser. Returns `Err` if the
    /// browser doesn't expose WebGPU, no adapter is available, or
    /// device creation fails — caller should fall back to Canvas2D.
    pub async fn new(canvas: HtmlCanvasElement) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });

        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| format!("WebGpuBackend: create_surface failed: {e:?}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| {
                "WebGpuBackend: no GPU adapter available — falling back to Canvas2D".to_string()
            })?;

        // Pick the texture-array depth before requesting the device:
        // wgpu only honors `max_texture_array_layers` up to whatever we
        // declare in `required_limits`. The adapter advertises a hard
        // ceiling (typically 2048 on desktop, 256 on the WebGPU MVP
        // floor); we clamp it into [`ATLAS_LAYERS_MIN`, `ATLAS_LAYERS_MAX`]
        // so memory stays bounded while still giving Claude-style TUIs
        // (which mix CJK + box-drawing + spinner glyphs) enough cache
        // headroom to avoid LRU thrash.
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
            .map_err(|e| format!("WebGpuBackend: request_device failed: {e:?}"))?;

        let surface_caps = surface.get_capabilities(&adapter);
        // Prefer an sRGB format so theme colors map intuitively. Fall
        // back to whatever the surface advertises first if no sRGB
        // option exists (rare but possible on exotic hardware).
        let format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 1,
            height: 1,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // ─── Shader + bind-group layout + render pipeline ────────────
        // Compile the WGSL cell shader. `include_str!` baked at
        // compile time so the binary doesn't need to read the .wgsl
        // file at runtime.
        let cell_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ridge-cell-shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shaders/cell.wgsl"
            ))),
        });

        // Bind group layout matches WGSL @group(0): uniform buffer
        // (FrameUniform) + texture_2d_array<f32> + sampler.
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

        // Vertex buffer layout — one buffer of CellInstance, stepped
        // per-instance. Field offsets must match the #[repr(C)] struct
        // declaration above so the WGSL @location attributes pull from
        // the right bytes.
        let instance_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: CELL_INSTANCE_STRIDE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                }, // cell_xy
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                }, // cell_size
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                }, // atlas_uv
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Uint32,
                }, // atlas_layer
                wgpu::VertexAttribute {
                    offset: 36,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x4,
                }, // fg_rgba
                wgpu::VertexAttribute {
                    offset: 52,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                }, // bg_rgba
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
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        // ─── GPU resource allocation ─────────────────────────────────
        // Compute the initial slot dimensions from the default
        // FrameMetrics (cell_w=8, cell_h=16, dpr=1.0). The first
        // `begin_frame` call from the renderer will provide actual
        // metrics; if they require a larger slot, the atlas + rasterizer
        // are rebuilt then. For default metrics this evaluates to the
        // historical 64 × 96 baseline.
        let initial_metrics = FrameMetrics {
            cell_w: 8.0,
            cell_h: 16.0,
            dpr: 1.0,
        };
        let (slot_w, slot_h) = Self::slot_dims_for(&initial_metrics);

        // Glyph atlas: D2 texture array, `atlas_layers` layers, RGBA8.
        // Format must be sRGB-aware so the sampled coverage carries
        // through linearly without extra gamma fixup in the shader.
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
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("ridge-atlas-view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ridge-atlas-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            // Nearest filtering preserves the browser's already-AA'd glyph
            // alpha exactly (the rasterizer's `fill_text` produces hinted +
            // anti-aliased coverage). Linear sampling on top blurs box-
            // drawing characters and produces a half-transparent edge at
            // the UV-cropped bbox boundary because it interpolates with
            // the cleared (transparent) area beyond. Nearest avoids both
            // and is what most terminal emulators use for cell rendering.
            // (User report 2026-05-05: 字符画细线 / 列间残留缝隙.)
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // FrameUniform { viewport: vec2<f32>, _pad: vec2<f32> } — 16 bytes.
        let frame_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ridge-frame-uniform"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Per-instance vertex buffer.
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ridge-instance-buffer"),
            size: (INITIAL_INSTANCE_CAPACITY as u64) * CELL_INSTANCE_STRIDE,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ridge-cell-bg"),
            layout: &cell_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        // OffscreenCanvas-based rasterizer sized to match the atlas
        // slot exactly so RasterizedGlyph.rgba can be written into a
        // texture layer via queue.write_texture without cropping.
        let rasterizer = GlyphRasterizer::new(slot_w as u16, slot_h as u16)?;

        Ok(Self {
            surface,
            device,
            queue,
            config,
            cell_shader,
            cell_bind_group_layout,
            cell_pipeline,
            atlas_texture,
            atlas_view,
            sampler,
            frame_uniform,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            bind_group,
            rasterizer,
            pending_instances: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY as usize),
            // Layer 0 reserved (§4.5.d) — start handing out from 1.
            next_free_layer: ATLAS_RESERVED_LAYERS,
            atlas_layers,
            // One slot per texture-array layer; reset every begin_frame.
            // `vec![false; n]` is the only allocation; subsequent frames
            // reuse the buffer in place.
            frame_pinned: vec![false; atlas_layers as usize],
            font_family: String::from("monospace"),
            font_size_px: 15.0,
            // GlyphAtlas capacity matches the USABLE layer count so the
            // LRU's eviction trigger fires exactly when GPU slots are
            // exhausted — never trying to evict the reserved layer 0.
            atlas: GlyphAtlas::new((atlas_layers - ATLAS_RESERVED_LAYERS) as usize),
            slot_w,
            slot_h,
            metrics: initial_metrics,
            theme: Theme::default_dark(),
        })
    }

    /// Set the CSS font family + pixel size used for glyph
    /// rasterization. Mirrors `Canvas2dBackend::set_font` (a non-trait
    /// method). Caller (eventually `RenderHandle::configure` in §4.1.e)
    /// invokes this whenever the user picks a new terminal font.
    ///
    /// Changing size or family invalidates the GlyphAtlas — even though
    /// `font_family_hash` + `font_size_q` on `GlyphKey` already
    /// disambiguate cache entries by metrics, the texture-array layers
    /// occupied by the OLD-size entries remain bound until natural LRU
    /// rotation. On a sudden DPR / font-size change every visible cell
    /// becomes a miss simultaneously, the atlas hits its capacity, the
    /// evict-and-reuse path engages mid-frame, and the user sees one or
    /// two frames of bg-only fallback ("missing characters" /
    /// "字符位置错乱" right after window resize). Eagerly clearing LRU
    /// + resetting `next_free_layer` lets new-size glyphs land in slot
    /// 0 immediately on the next frame.
    pub fn set_font_config(&mut self, font_family: String, font_size_px: f32) {
        let size_changed = (self.font_size_px - font_size_px).abs() > 0.01;
        let family_changed = self.font_family != font_family;
        self.font_family = font_family;
        self.font_size_px = font_size_px;
        if size_changed || family_changed {
            self.invalidate_atlas();
        }
    }

    /// Compute the device-pixel atlas slot size required for the given
    /// metrics. Wide CJK cells need ≥ `cell_w × dpr × 2` device pixels
    /// horizontally so the rasterizer's OffscreenCanvas can hold the
    /// full advance without clipping. `slot_w` is rounded up to the
    /// next power of two so `bytes_per_row = slot_w × 4` always
    /// satisfies wgpu's `COPY_BYTES_PER_ROW_ALIGNMENT` of 256 bytes
    /// (i.e. slot_w must be a multiple of 64 — power-of-two starting
    /// at 64 covers all valid sizes).
    ///
    /// Vertically we add a 25% safety margin over `cell_h × dpr` to
    /// catch font_bounding_box descenders, italics overhang, and
    /// stacked combining marks. No alignment requirement on slot_h.
    fn slot_dims_for(metrics: &FrameMetrics) -> (u32, u32) {
        let cell_w_dev = (metrics.cell_w * metrics.dpr).max(1.0);
        let cell_h_dev = (metrics.cell_h * metrics.dpr).max(1.0);
        let wide_w_dev = (cell_w_dev * 2.0).ceil() as u32;
        let row_h_dev = cell_h_dev.ceil() as u32;
        let slot_w = wide_w_dev.max(ATLAS_SLOT_W_FLOOR).next_power_of_two();
        let slot_h = (row_h_dev + row_h_dev / 4).max(ATLAS_SLOT_H_FLOOR);
        (slot_w, slot_h)
    }

    /// Rebuild the GPU atlas resources at the current `slot_w` /
    /// `slot_h`. Called from `begin_frame` when new metrics demand a
    /// slot larger than the existing allocation. All cached glyphs are
    /// dropped — the next frame's `draw_row` re-rasterizes them at the
    /// new slot dimensions. Bounded cost: at most one rebuild per
    /// metric change (font / DPR / cell-size), and the renderer's
    /// `requires_full_frame() == true` ensures every visible row gets
    /// re-emitted on the very next tick so the user never sees a
    /// half-populated atlas.
    fn rebuild_atlas(&mut self) -> Result<(), String> {
        // Drop the LRU cache contents — every entry's (layer, uv) are
        // about to become stale because the texture array is being
        // reallocated.
        self.atlas.clear();
        self.next_free_layer = ATLAS_RESERVED_LAYERS;

        // Re-allocate the texture array at the new slot dimensions.
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
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("ridge-atlas-view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        // Bind group references the atlas view directly; the old
        // bind_group still holds a reference to the OLD view, so it
        // must be replaced before the next end_frame submission.
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ridge-cell-bg"),
            layout: &self.cell_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.frame_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        // Rasterizer's OffscreenCanvas dimensions must match the slot
        // exactly so its `get_image_data` output is `slot_w × slot_h
        // × 4` bytes — same shape `queue.write_texture` expects.
        let rasterizer = GlyphRasterizer::new(self.slot_w as u16, self.slot_h as u16)?;

        self.atlas_texture = atlas_texture;
        self.atlas_view = atlas_view;
        self.bind_group = bind_group;
        self.rasterizer = rasterizer;
        Ok(())
    }
}

impl RenderBackend for WebGpuBackend {
    fn measure_font(&self, font_family: &str, font_size_px: f32) -> Result<(f32, f32), String> {
        // Delegate to the OffscreenCanvas-backed rasterizer (§4.5.a).
        // Same `measure_text("M")` + `size * 1.4` heuristic Canvas2dBackend
        // uses, just on the rasterizer's already-allocated 2D context —
        // so cellW/cellH agrees across backends bit-for-bit and fitPane
        // stays backend-agnostic.
        self.rasterizer.measure(font_family, font_size_px)
    }

    fn requires_full_frame(&self) -> bool {
        // `LoadOp::Clear(theme.bg)` in `end_frame` wipes the entire
        // swap-chain texture every frame, so non-dirty rows from the
        // previous frame don't survive the clear. The renderer must
        // re-emit every visible row through `draw_row` each tick —
        // that's what this hook signals. Canvas2D returns the default
        // false because its partial-clear (`fill_rect` only on dirty
        // rows) preserves un-touched pixels.
        true
    }

    fn resize_surface(&mut self, width_css: u32, height_css: u32, dpr: f32) -> Result<(), String> {
        let backing_w = ((width_css as f32) * dpr).round().max(1.0) as u32;
        let backing_h = ((height_css as f32) * dpr).round().max(1.0) as u32;
        if self.config.width != backing_w || self.config.height != backing_h {
            self.config.width = backing_w;
            self.config.height = backing_h;
            self.surface.configure(&self.device, &self.config);
        }
        Ok(())
    }

    fn invalidate_atlas(&mut self) {
        // Drop every cached `(GlyphKey, GlyphEntry)` mapping and reset
        // the texture-array layer pointer back to the first usable
        // layer (= `ATLAS_RESERVED_LAYERS`; layer 0 is the permanent
        // transparent fallback for atlas-miss cells, see §4.5.d).
        // Texture pixels are NOT zero-filled — the next
        // `queue.write_texture` call overwrites whatever bytes lived
        // in the slot. Anything still sampling those slots from a
        // stale instance buffer would alias, but `pending_instances`
        // is cleared every `begin_frame` and the renderer's
        // `full_redraw_pending` guarantees the next frame re-pushes
        // every visible cell with fresh atlas hits — so by the time
        // the GPU samples, the new glyph bitmaps are already uploaded.
        self.atlas.clear();
        self.next_free_layer = ATLAS_RESERVED_LAYERS;
    }

    fn begin_frame(&mut self, metrics: FrameMetrics, theme: &Theme) {
        // Record per-frame state + reset the cell-instance accumulator.
        // Vec::clear keeps capacity, so once steady-state is reached
        // the per-frame allocator cost is zero.
        self.metrics = metrics;
        self.theme = theme.clone();
        self.pending_instances.clear();
        // Reset per-layer pin flags. Each `draw_row` call will pin the
        // layers it references so the eviction path can avoid clobbering
        // them mid-frame. Filling in place keeps the allocation; cost is
        // O(atlas_layers) ≈ 1 µs at 1024 layers.
        for p in &mut self.frame_pinned {
            *p = false;
        }

        // Atlas slot tracks current metrics. Grow only — shrinking on
        // every small metric jiggle would thrash the rasterizer's
        // OffscreenCanvas allocation. Once slot_w / slot_h are big
        // enough, they stay there for the life of this backend.
        let (need_w, need_h) = Self::slot_dims_for(&self.metrics);
        if need_w > self.slot_w || need_h > self.slot_h {
            self.slot_w = need_w.max(self.slot_w);
            self.slot_h = need_h.max(self.slot_h);
            // Best-effort rebuild. On failure we keep the old (now
            // undersized) atlas — wide glyphs continue to clip but
            // the renderer doesn't crash. The error path is rare:
            // OffscreenCanvas / get_context only fail under exotic
            // browser conditions, and at that point this pane already
            // had a working atlas one frame earlier.
            let _ = self.rebuild_atlas();
        }
    }

    fn clear(&mut self) {
        // Records intent only — actual GPU work happens in end_frame()
        // so a single RenderPass can include both the clear AND the
        // draw_row instance draws. The renderer's call sequence is
        // begin_frame → clear → draw_row* → cursor/overlay/underline →
        // end_frame; clear() runs BEFORE draw_row, so it'd be wasteful
        // to acquire a swap-chain texture here only to wait for the
        // per-row instances to accumulate before drawing them.
        //
        // We always clear with theme.bg; no flag needs to be tracked.
    }

    fn draw_row(&mut self, row: &RowDraw<'_>, attrs_table: &AttrTable) {
        // Per-cell instance accumulation with atlas lookup. For each
        // non-continuation cell:
        //   1. Compute GlyphKey from (font hash, size, codepoint, style).
        //   2. atlas.lookup → on hit, push CellInstance with entry's
        //      layer + uv.
        //   3. On miss: if layer pool isn't full, rasterize via the
        //      OffscreenCanvas, queue.write_texture into the next free
        //      layer, atlas.insert, push CellInstance. If full, fall
        //      back to bg-only (atlas_layer=0, atlas_uv=zero) — the
        //      shader samples empty content there, coverage=0, mix
        //      collapses to bg. Layer reuse on eviction lands in
        //      §4.1.c.glyph.eviction.
        let row_idx = row.row_index;
        let cell_w = self.metrics.cell_w * self.metrics.dpr;
        let cell_h = self.metrics.cell_h * self.metrics.dpr;
        let theme = self.theme.clone();
        // Integer-align row top + bottom so adjacent rows share an exact
        // pixel boundary with no fractional gap or overlap. cell_h can
        // be fractional when DPR is non-integer (e.g., 1.25, 1.5) and
        // the resulting fractional-pixel column quads expose thin seams
        // between adjacent rows / cells in box-drawing characters.
        let row_top = ((row_idx as f32) * cell_h).floor();
        let row_bot = (((row_idx + 1) as f32) * cell_h).floor();
        let row_h_int = (row_bot - row_top).max(1.0);
        // Pre-compute a stable hash for the current font family so
        // every cell of the current frame keys to the same bucket.
        let font_family_hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            self.font_family.hash(&mut h);
            h.finish()
        };
        // Quantize size to 1/100 px (per GlyphKey docstring).
        let font_size_q = (self.font_size_px * 100.0).round() as u16;

        for (col, cell) in row.cells.iter().enumerate() {
            if cell.width == 0 {
                continue;
            }
            let attrs = attrs_table.get(cell.attr);
            let (_attrs, fg, bg) =
                crate::render::backend::resolve_cell_colors(cell, attrs_table, &theme);
            // Integer-aligned column boundaries — same scheme as row_top /
            // row_bot above. Cell width is derived from the integer
            // boundaries (NOT from `cell_w * cell.width`), which keeps
            // adjacent cells flush even when `cell_w` is fractional.
            let cell_span = cell.width.max(1) as usize;
            let pixel_x = ((col as f32) * cell_w).floor();
            let pixel_x_right = (((col + cell_span) as f32) * cell_w).floor();
            let cell_w_px = (pixel_x_right - pixel_x).max(1.0);
            let pixel_y = row_top;

            // Style flags: pack BOLD + ITALIC bits per GlyphKey docstring.
            let mut style_flags: u8 = 0;
            if attrs.flags.contains(crate::term::attrs::Flags::BOLD) {
                style_flags |= GlyphKey::STYLE_BOLD;
            }
            if attrs.flags.contains(crate::term::attrs::Flags::ITALIC) {
                style_flags |= GlyphKey::STYLE_ITALIC;
            }

            let key = GlyphKey {
                font_family_hash,
                font_size_q,
                glyph_id: cell.ch as u32,
                style_flags,
            };

            // Try the LRU atlas. On hit (and on first paint of this
            // glyph in this session) `entry` carries the texture-array
            // layer index + UV.
            let entry = if let Some(e) = self.atlas.lookup(&key) {
                // Pin the hit layer so a later miss in the same frame
                // can't evict + overwrite it before end_frame submits.
                if (e.layer as usize) < self.frame_pinned.len() {
                    self.frame_pinned[e.layer as usize] = true;
                }
                Some(e)
            } else {
                // Miss. Rasterize first; if that fails we bail to bg-only.
                // Pass DPR so the rasterizer paints at device-pixel scale
                // (§7.2 fix: glyphs were rendering at CSS-px size on a
                // device-px slot, so the cell quad sampled mostly-empty
                // texture and the user saw tiny / thin text).
                // Pass the same style_flags used for the GlyphKey so the
                // browser actually paints BOLD/ITALIC variants (§4.5.b).
                match self.rasterizer.rasterize(
                    &self.font_family,
                    self.font_size_px,
                    self.metrics.dpr,
                    style_flags,
                    cell.ch,
                ) {
                    Ok(glyph) => {
                        // Pick a target layer:
                        //   1. Fresh slot if any free (pre-eviction phase
                        //      while next_free_layer hasn't filled).
                        //   2. Else evict the oldest *unpinned* LRU entry
                        //      and reuse its layer — pinning prevents the
                        //      in-frame race where we'd reuse a layer
                        //      already cited by an earlier instance.
                        //   3. If every layer is pinned (visible-unique-
                        //      glyph count > capacity, vanishingly rare
                        //      after Fix B), fall through to bg-only.
                        let chosen: Option<u32> = if self.next_free_layer < self.atlas_layers {
                            let l = self.next_free_layer;
                            self.next_free_layer += 1;
                            Some(l)
                        } else {
                            pick_evictable_layer(&mut self.atlas, &self.frame_pinned)
                        };
                        match chosen {
                            Some(layer) => {
                                // Pin BEFORE write_texture so a later miss
                                // in this same frame can't reclaim the
                                // layer we're about to fill.
                                if (layer as usize) < self.frame_pinned.len() {
                                    self.frame_pinned[layer as usize] = true;
                                }
                                // Upload only the bbox region; the rest of
                                // the slot stays cleared from prior frames
                                // (texture is allocated zero-filled). This
                                // also keeps bytes_per_row aligned to
                                // glyph.width × 4.
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
                                        // Source data is the full slot
                                        // (Vec<u8> length == slot_w ×
                                        // slot_h × 4); upload covers the
                                        // bbox region. Stride matches the
                                        // source row width = self.slot_w.
                                        bytes_per_row: Some(self.slot_w * 4),
                                        rows_per_image: Some(self.slot_h),
                                    },
                                    wgpu::Extent3d {
                                        width: self.slot_w,
                                        height: self.slot_h,
                                        depth_or_array_layers: 1,
                                    },
                                );
                                // Crop UV to the actual glyph bounding
                                // box. The cell quad samples [u0,v0] →
                                // [u1,v1]; outside this rect the texture
                                // is empty (transparent), so without the
                                // crop the cell over-samples a mostly-
                                // empty slot and stretches the glyph into
                                // the upper-left corner of the cell.
                                let u1 = (glyph.width as f32) / (self.slot_w as f32);
                                let v1 = (glyph.height as f32) / (self.slot_h as f32);
                                let new_entry = GlyphEntry {
                                    layer: layer as u16,
                                    uv: [0.0, 0.0, u1, v1],
                                    advance: glyph.advance,
                                    ascent_offset: glyph.ascent_offset,
                                    px_w: glyph.width,
                                    px_h: glyph.height,
                                };
                                self.atlas.insert(key, new_entry);
                                Some(new_entry)
                            }
                            // Every layer pinned this frame — bg-only.
                            None => None,
                        }
                    }
                    Err(_) => None, // rasterize failure → bg-only
                }
            };

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
                // visible symptom users hit when slot was also too
                // narrow was "中文只有左半边" (Step 1 stops the slot
                // truncation; Step 2 stops the residual 20% horizontal
                // stretch and brings WebGPU output to pixel-parity with
                // Canvas2D's `fill_text` left-aligned glyph rendering).
                //
                // Background instance: atlas_layer=0 (reserved
                // transparent layer) + atlas_uv=zero → shader samples
                // alpha 0, `mix(bg, fg, 0) == bg` → opaque cell bg
                // covering the full 2-cell rect.
                self.pending_instances.push(CellInstance {
                    cell_xy: [pixel_x, pixel_y],
                    cell_size: [cell_w_px, row_h_int],
                    atlas_uv: [0.0, 0.0, 0.0, 0.0],
                    atlas_layer: 0,
                    fg_rgba: rgba_u8_to_f32(fg),
                    bg_rgba: rgba_u8_to_f32(bg),
                });
                if let Some(e) = entry {
                    // Glyph natural width in device pixels (advance.ceil
                    // from the rasterizer, already clamped to slot_w).
                    // Cap at the cell quad width so an unusually wide
                    // glyph doesn't bleed past the cell into the next
                    // column. Left-align (cell_xy.x = pixel_x) to match
                    // Canvas2D's `fill_text` baseline placement.
                    let glyph_w_px = (e.px_w as f32).min(cell_w_px).max(1.0);
                    // bg=transparent so this instance composites as
                    // premultiplied fg over the bg instance via the
                    // pipeline's ALPHA_BLENDING — `mix(0, fg, coverage)`
                    // already yields premultiplied output.
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
                // instance because the natural advance fits the cell
                // quad (or we don't know any better — bbox is already
                // ≤ 1 cell). The shader's `mix(bg, fg, coverage)` paints
                // the glyph over the cell bg in one pass.
                self.pending_instances.push(CellInstance {
                    cell_xy: [pixel_x, pixel_y],
                    // Use integer row height so adjacent rows share an
                    // exact pixel boundary; same for column width.
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
        // Cursor reuses the cell pipeline — geometrically it's just
        // another colored quad at (cursor.row, cursor.col), drawn
        // OVER the row instances that draw_row already pushed for
        // that cell. Draw order = instance order in pending_instances,
        // so pushing the cursor here (after all draw_row calls) puts
        // it on top.
        //
        // Style:
        //   Block       — fills the full cell with cursor_color
        //   Bar         — 2-px-wide vertical strip at left edge
        //   Underline   — 2-px-tall horizontal strip at bottom edge
        //
        // §4.1.d.cursor_text: paint cursor.ch on top of the block in
        // cursor_text_color (xterm's "inverse cursor"). Done here too:
        // a SECOND instance pulls the glyph from the atlas and uses
        // cursor_text_color as fg + cursor_color as bg.
        use crate::render::backend::CursorStyle;

        let cell_w = self.metrics.cell_w * self.metrics.dpr;
        let cell_h = self.metrics.cell_h * self.metrics.dpr;
        // Integer-aligned cell box (same scheme as draw_row).
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

        // 2) Inverted glyph (only meaningful for Block — Bar / Underline
        //    don't cover the cell so the underlying glyph is still
        //    visible from the row's CellInstance push). For Block, we
        //    look up cursor.ch in the atlas and push a CellInstance
        //    with bg=cursor_color, fg=cursor_text_color so the glyph
        //    renders inverted on top of the cursor block.
        if matches!(cursor.style, CursorStyle::Block) && cursor.ch != ' ' {
            let font_family_hash = {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut h = DefaultHasher::new();
                self.font_family.hash(&mut h);
                h.finish()
            };
            let font_size_q = (self.font_size_px * 100.0).round() as u16;
            let key = GlyphKey {
                font_family_hash,
                font_size_q,
                glyph_id: cursor.ch as u32,
                style_flags: 0, // cursor uses default style; bold cursor not modeled
            };
            // Atlas hit only — we don't rasterize-on-miss inside
            // draw_cursor to keep the per-frame work bounded. If the
            // glyph isn't cached yet, the cursor renders as a solid
            // block this frame; the subsequent draw_row tick will
            // populate the atlas, and the next frame's cursor draw
            // gets the inverted glyph.
            if let Some(entry) = self.atlas.lookup(&key) {
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
        // Selection overlay reuses the cell pipeline — each
        // (row, col_start, col_end) rect becomes one CellInstance
        // covering the full cell-height with `selection_bg` (which
        // carries its own alpha, typically 0x3d-0x60 for "translucent
        // tint"). Alpha blending in the pipeline composites it over
        // the cell content draw_row already pushed.
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
            // Integer-aligned overlay box — same boundary scheme as
            // draw_row so the translucent tint flush-aligns with the
            // cell content beneath, with no fractional-pixel halo.
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
        // Hyperlink underlines: a 2-px-tall (DPR-scaled) strip at
        // cell-bottom for each (row, col_start, col_end) span. Solid
        // color from theme.hyperlink_color.
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
            // Integer-aligned underline strip flush with cell boundaries.
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
        // Unified per-frame submit. Steps:
        //   1. Upload frame uniform (viewport in pixels).
        //   2. Grow instance buffer if the frame exceeded current capacity.
        //   3. Upload pending CellInstance bytes.
        //   4. Acquire swap-chain texture (bail on surface-lost).
        //   5. Single RenderPass with LoadOp::Clear(theme.bg) +
        //      pipeline + bind group + vertex buffer + indirect-style
        //      `draw(0..4, 0..N_cells)`.
        //   6. Submit + present.

        // 1) Frame uniform: viewport in pixels (post-DPR).
        let viewport: [f32; 4] = [
            self.config.width as f32,
            self.config.height as f32,
            0.0,
            0.0,
        ];
        // `[f32; 4]` is Pod (Zeroable + transmute-safe); cast via bytemuck
        // instead of an unsafe transmute (§4.5.c).
        self.queue
            .write_buffer(&self.frame_uniform, 0, bytemuck::bytes_of(&viewport));

        // 2-3) Instance buffer.
        let n_cells = self.pending_instances.len() as u32;
        if n_cells > 0 {
            // Grow on overflow. Doubling keeps amortized cost O(1)
            // per cell across a session.
            if n_cells > self.instance_capacity {
                let new_capacity = n_cells.next_power_of_two().max(self.instance_capacity * 2);
                self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("ridge-instance-buffer-grown"),
                    size: (new_capacity as u64) * CELL_INSTANCE_STRIDE,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.instance_capacity = new_capacity;
                // Bind group still references the OLD instance buffer
                // by virtue of the binding pointing only at frame_uniform
                // + atlas_view + sampler — instance buffer is bound
                // per-frame via set_vertex_buffer below, so there's
                // nothing to rebuild here. ✅
            }
            // Bytes view — CellInstance derives bytemuck::Pod, so
            // `cast_slice` is checked at compile time without unsafe
            // (§4.5.c).
            let instance_bytes: &[u8] = bytemuck::cast_slice(&self.pending_instances);
            self.queue
                .write_buffer(&self.instance_buffer, 0, instance_bytes);
        }

        // 4) Swap-chain texture.
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(_e) => {
                // Surface lost / outdated — bail this frame; the
                // renderer's full_redraw_pending will retry on the
                // next tick once resize_surface reconfigures.
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // 5) Single command encoder + render pass.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ridge-term-frame-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ridge-term-frame-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(rgba_to_wgpu_color(self.theme.bg)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if n_cells > 0 {
                pass.set_pipeline(&self.cell_pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
                // 4 vertices per quad (TriangleStrip), N_cells instances.
                pass.draw(0..4, 0..n_cells);
            }
        }

        // 6) Submit + present.
        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }
}
