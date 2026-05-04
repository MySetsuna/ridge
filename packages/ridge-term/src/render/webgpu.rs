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
use super::glyph_atlas::GlyphAtlas;
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
        // Record per-frame state. Actual GPU work happens in
        // `clear()` + `end_frame()` for this slice.
        self.metrics = metrics;
        self.theme = theme.clone();
    }

    fn clear(&mut self) {
        // Acquire current swap-chain texture and submit a single
        // RenderPass that wipes the surface with `theme.bg`. The pass
        // ends immediately — per-row / cursor / overlay paints land
        // in §4.1.b+ as separate pipeline passes.
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(_e) => {
                // Surface lost / outdated. The renderer's
                // full_redraw_pending flag will retry on the next tick
                // once the surface is reconfigured (which the caller
                // must arrange via `resize_surface`). We deliberately
                // don't log to console.warn here to avoid coupling on
                // an extra web-sys feature; future slices may add it
                // when the WebGpuBackend gets richer instrumentation.
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ridge-term-clear-encoder"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ridge-term-clear-pass"),
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
            // Pass dropped here — no draws, just the clear.
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }

    fn draw_row(&mut self, _row: &RowDraw<'_>, _attrs_table: &AttrTable) {
        // No-op for §4.1 slice 1. §4.1.b lands real glyph rasterization
        // + texture-array atlas upload + per-row instance buffer.
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
        // Slice 1: clear() already submitted + presented. Future
        // slices will refactor so begin_frame opens a single pass
        // and end_frame closes it after all draws.
    }
}
