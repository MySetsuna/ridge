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

use crate::render::backend::{
    CursorDraw, FrameMetrics, RenderBackend, RowDraw, Theme,
};
use crate::term::attr_table::AttrTable;
use super::glyph_atlas::{GlyphAtlas, GlyphEntry, GlyphKey};
use super::glyph_rasterizer::GlyphRasterizer;
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
    /// (`ATLAS_SLOT_W`, `ATLAS_SLOT_H`) so its output bitmap fits
    /// exactly into one atlas-texture layer with no clipping.
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
    /// glyph admitted to the atlas; once it reaches `ATLAS_LAYERS`
    /// new misses fall back to bg-only rendering.
    ///
    /// §4.1.c.glyph.eviction (future) extends GlyphAtlas to return
    /// the evicted entry's layer so this counter can be replaced by
    /// proper layer-reuse-on-eviction.
    next_free_layer: u32,
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
    metrics: FrameMetrics,
    theme: Theme,
}

/// Slot dimensions in device pixels — generous square covering most
/// cell sizes × DPR. Future iteration can make this configurable per
/// font size; for the §4.1.c slice a fixed slot is fine.
const ATLAS_SLOT_W: u32 = 32;
const ATLAS_SLOT_H: u32 = 32;
/// Number of texture-array layers = GlyphAtlas LRU capacity.
/// `Limits::downlevel_defaults().max_texture_array_layers == 256` so
/// this is the safe portable baseline.
const ATLAS_LAYERS: u32 = 256;
/// Initial per-frame cell instance buffer capacity. Realistic terminal
/// sessions have a few thousand cells; 1024 covers small panes and the
/// buffer grows on demand for larger ones.
const INITIAL_INSTANCE_CAPACITY: u32 = 1024;

/// CPU-side instance struct matching the WGSL `InstanceIn` layout.
/// `#[repr(C)]` makes the field order load-bearing — must mirror the
/// `attributes: &[VertexAttribute { offset, ... }]` array passed to
/// `RenderPipelineDescriptor::vertex.buffers`.
///
/// Future iteration will populate a Vec<CellInstance> per frame from
/// the renderer's dirty rows, then `queue.write_buffer` it before the
/// indirect draw.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // Populated by §4.1.c draw_row body in a future iteration.
struct CellInstance {
    cell_xy: [f32; 2],     // 0..8
    cell_size: [f32; 2],   // 8..16
    atlas_uv: [f32; 4],    // 16..32
    atlas_layer: u32,      // 32..36
    fg_rgba: [f32; 4],     // 36..52
    bg_rgba: [f32; 4],     // 52..68
}

const CELL_INSTANCE_STRIDE: u64 = std::mem::size_of::<CellInstance>() as u64;

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
                "WebGpuBackend: no GPU adapter available — falling back to Canvas2D"
                    .to_string()
            })?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("ridge-term-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults(),
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
            source: wgpu::ShaderSource::Wgsl(
                std::borrow::Cow::Borrowed(include_str!("shaders/cell.wgsl")),
            ),
        });

        // Bind group layout matches WGSL @group(0): uniform buffer
        // (FrameUniform) + texture_2d_array<f32> + sampler.
        let cell_bind_group_layout = device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
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
            },
        );

        let pipeline_layout = device.create_pipeline_layout(
            &wgpu::PipelineLayoutDescriptor {
                label: Some("ridge-cell-pipeline-layout"),
                bind_group_layouts: &[&cell_bind_group_layout],
                push_constant_ranges: &[],
            },
        );

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

        let cell_pipeline = device.create_render_pipeline(
            &wgpu::RenderPipelineDescriptor {
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
            },
        );

        // ─── GPU resource allocation ─────────────────────────────────
        // Glyph atlas: D2 texture array, ATLAS_LAYERS layers, RGBA8.
        // Format must be sRGB-aware so the sampled coverage carries
        // through linearly without extra gamma fixup in the shader.
        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ridge-atlas-texture"),
            size: wgpu::Extent3d {
                width: ATLAS_SLOT_W,
                height: ATLAS_SLOT_H,
                depth_or_array_layers: ATLAS_LAYERS,
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
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
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
        let rasterizer = GlyphRasterizer::new(ATLAS_SLOT_W as u16, ATLAS_SLOT_H as u16)?;

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
            next_free_layer: 0,
            font_family: String::from("monospace"),
            font_size_px: 15.0,
            // GlyphAtlas capacity must equal ATLAS_LAYERS so atlas
            // eviction matches GPU layer reuse exactly.
            atlas: GlyphAtlas::new(ATLAS_LAYERS as usize),
            metrics: FrameMetrics {
                cell_w: 8.0,
                cell_h: 16.0,
                dpr: 1.0,
            },
            theme: Theme::default_dark(),
        })
    }
}

impl RenderBackend for WebGpuBackend {
    fn measure_font(
        &self,
        _font_family: &str,
        _font_size_px: f32,
    ) -> Result<(f32, f32), String> {
        // §4.1.b will return real metrics from the glyph rasterizer.
        // For the bg-only slice we return a sentinel; consumers will
        // route font-measurement queries through Canvas2D until then.
        Err("WebGpuBackend::measure_font not implemented — defer to Canvas2D".to_string())
    }

    fn resize_surface(
        &mut self,
        width_css: u32,
        height_css: u32,
        dpr: f32,
    ) -> Result<(), String> {
        let backing_w = ((width_css as f32) * dpr).round().max(1.0) as u32;
        let backing_h = ((height_css as f32) * dpr).round().max(1.0) as u32;
        if self.config.width != backing_w || self.config.height != backing_h {
            self.config.width = backing_w;
            self.config.height = backing_h;
            self.surface.configure(&self.device, &self.config);
        }
        Ok(())
    }

    fn begin_frame(&mut self, metrics: FrameMetrics, theme: &Theme) {
        // Record per-frame state + reset the cell-instance accumulator.
        // Vec::clear keeps capacity, so once steady-state is reached
        // the per-frame allocator cost is zero.
        self.metrics = metrics;
        self.theme = theme.clone();
        self.pending_instances.clear();
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
            let (_attrs, fg, bg) = crate::render::backend::resolve_cell_colors(
                cell, attrs_table, &theme,
            );
            let cell_w_px = cell_w * cell.width.max(1) as f32;
            let pixel_x = (col as f32) * cell_w;
            let pixel_y = (row_idx as f32) * cell_h;

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
                Some(e)
            } else if self.next_free_layer < ATLAS_LAYERS {
                // Miss with room. Rasterize → upload → insert.
                match self.rasterizer.rasterize(
                    &self.font_family,
                    self.font_size_px,
                    cell.ch,
                ) {
                    Ok(glyph) => {
                        let layer = self.next_free_layer;
                        self.next_free_layer += 1;
                        self.queue.write_texture(
                            wgpu::ImageCopyTexture {
                                texture: &self.atlas_texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                                aspect: wgpu::TextureAspect::All,
                            },
                            &glyph.rgba,
                            wgpu::ImageDataLayout {
                                offset: 0,
                                bytes_per_row: Some(glyph.width as u32 * 4),
                                rows_per_image: Some(glyph.height as u32),
                            },
                            wgpu::Extent3d {
                                width: glyph.width as u32,
                                height: glyph.height as u32,
                                depth_or_array_layers: 1,
                            },
                        );
                        let new_entry = GlyphEntry {
                            layer: layer as u16,
                            uv: [0.0, 0.0, 1.0, 1.0],
                            advance: glyph.advance,
                            ascent_offset: glyph.ascent_offset,
                            px_w: glyph.width,
                            px_h: glyph.height,
                        };
                        self.atlas.insert(key, new_entry);
                        Some(new_entry)
                    }
                    Err(_) => None, // rasterize failure → bg-only
                }
            } else {
                // Atlas full + no eviction support yet → bg-only fallback.
                None
            };

            let (atlas_uv, atlas_layer) = match entry {
                Some(e) => (e.uv, e.layer as u32),
                None => ([0.0, 0.0, 0.0, 0.0], 0),
            };

            self.pending_instances.push(CellInstance {
                cell_xy: [pixel_x, pixel_y],
                cell_size: [cell_w_px, cell_h],
                atlas_uv,
                atlas_layer,
                fg_rgba: rgba_u8_to_f32(fg),
                bg_rgba: rgba_u8_to_f32(bg),
            });
        }
    }

    fn draw_cursor(&mut self, _cursor: &CursorDraw, _attrs_table: &AttrTable) {
        // No-op; §4.1.c lands cursor pipeline.
    }

    fn draw_selection_overlay(&mut self, _rects: &[(usize, usize, usize)]) {
        // No-op; §4.1.c lands overlay pipeline.
    }

    fn draw_hyperlink_underlines(&mut self, _rects: &[(usize, usize, usize)]) {
        // No-op; §4.1.c lands underline pipeline.
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
        let viewport_bytes: [u8; 16] = unsafe {
            std::mem::transmute::<[f32; 4], [u8; 16]>(viewport)
        };
        self.queue.write_buffer(&self.frame_uniform, 0, &viewport_bytes);

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
            // Bytes view — CellInstance is #[repr(C)] over Pod fields
            // (f32 / u32 / arrays), so the slice transmute is sound.
            // Future iteration may add bytemuck for safer-by-default.
            let instance_bytes: &[u8] = unsafe {
                std::slice::from_raw_parts(
                    self.pending_instances.as_ptr() as *const u8,
                    self.pending_instances.len() * std::mem::size_of::<CellInstance>(),
                )
            };
            self.queue.write_buffer(&self.instance_buffer, 0, instance_bytes);
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
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
