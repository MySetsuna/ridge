//! Shared swap-chain host — Round 3 §4.3 Phase B.
//!
//! Owns the single `wgpu::Surface` bound to the global `<canvas
//! data-rg-host>` element in `+page.svelte`. All `WebGpuPaneBackend`
//! instances funnel their per-frame draw calls through here via
//! [`SurfaceHost::record_pane`]; one `surface.get_current_texture()` /
//! `queue.submit` / `present` pair runs per frame regardless of pane
//! count.
//!
//! ## Coordinate convention
//!
//! Pane backends accumulate instances in **pane-local device-pixel
//! coordinates** — `cell_xy` is `(col_index * cell_w, row_index * cell_h)`
//! starting from 0,0 at the pane's top-left. The vertex shader in
//! `shaders/cell.wgsl` divides those by `frame.viewport` (a `vec2<f32>`
//! holding `pane.viewport.w` × `pane.viewport.h`) to produce NDC.
//!
//! The mapping from per-pane NDC to the host canvas's actual rect happens
//! at the GPU pipeline level: [`SurfaceHost::record_pane`] calls
//! `pass.set_viewport(x, y, w, h, 0, 1)` with the pane's scissor rect,
//! and `pass.set_scissor_rect` to clip overdraw at the boundaries. The
//! pane backend stays unaware of where on the host canvas it lives.
//!
//! ## Frame-store protocol
//!
//! Pane passes render into a persistent offscreen texture. The acquired
//! swap-chain texture is only a presentation target, so pane pixels do not
//! depend on WebView2 preserving `LoadOp::Load` contents across frames.
//! [`begin_frame`] seeds the persistent texture on every frame because
//! default terminal cells may be transparent; [`end_frame`] then blits the
//! complete frame to the current swap-chain image in one atomic present.

#![cfg(all(target_arch = "wasm32", feature = "webgpu"))]

use std::cell::RefCell;
use std::rc::Rc;

use web_sys::HtmlCanvasElement;

use super::gpu_context::{GpuContext, CANVAS_FORMAT, WALLPAPER_UNIFORM_SIZE};
use super::wallpaper::cover_uv_transform;

/// Pane viewport rectangle in **host-canvas device-pixel coordinates**.
/// `is_empty()` is true when the pane is parked-by-clip (pulled to zero
/// width by a splitter drag, or laid out entirely outside the host
/// canvas's bounds). Empty rects are skipped at `record_pane` so we
/// never call `set_viewport`/`set_scissor_rect` with zero extents
/// (wgpu validation rejects `width == 0 || height == 0`).
#[derive(Copy, Clone, Debug, Default)]
pub struct ScissorRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl ScissorRect {
    pub const ZERO: ScissorRect = ScissorRect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }
}

/// Convert a 0..255 RGBA byte tuple into the `wgpu::Color` form
/// `LoadOp::Clear` expects. wgpu treats the value as linear-space because
/// our surface format is `Bgra8Unorm` (no sRGB encoding at the ROP), so
/// the byte values land on the canvas unchanged — `theme.bg = #1e1e2e`
/// produces pixels at exactly `#1e1e2e`. Same convention as the per-pane
/// `rgba_to_wgpu_color` in `webgpu.rs`; duplicated here to keep the two
/// modules independent.
fn rgba_to_wgpu_color(rgba: [u8; 4]) -> wgpu::Color {
    wgpu::Color {
        r: (rgba[0] as f64) / 255.0,
        g: (rgba[1] as f64) / 255.0,
        b: (rgba[2] as f64) / 255.0,
        a: (rgba[3] as f64) / 255.0,
    }
}

const BLIT_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0,  1.0),
    );
    var uvs = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
    );
    var out: VertexOutput;
    out.position = vec4<f32>(positions[index], 0.0, 1.0);
    out.uv = uvs[index];
    return out;
}

@group(0) @binding(0) var frame_texture: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(frame_texture, frame_sampler, input.uv);
}
"#;

fn create_frame_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ridge-host-frame-store"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: CANVAS_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_blit_resources(
    device: &wgpu::Device,
    frame_view: &wgpu::TextureView,
) -> (
    wgpu::BindGroupLayout,
    wgpu::Sampler,
    wgpu::RenderPipeline,
    wgpu::BindGroup,
) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ridge-host-frame-blit-shader"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(BLIT_SHADER)),
    });
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("ridge-host-frame-blit-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("ridge-host-frame-blit-sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("ridge-host-frame-blit-pipeline-layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ridge-host-frame-blit-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
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
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: CANVAS_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ridge-host-frame-blit-bind-group"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(frame_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    (bind_group_layout, sampler, pipeline, bind_group)
}

/// Process-wide host-canvas swap-chain owner.
pub struct SurfaceHost {
    /// Borrowed reference to the shared GPU stack (device / queue /
    /// pipeline / atlas). Initialised before the host is constructed —
    /// `init` calls `GpuContext::get_or_init().await?` first so all
    /// resources share one Device.
    ctx: Rc<RefCell<GpuContext>>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Background color used by the seed-clear pass. Updated by
    /// `begin_frame` so theme changes propagate across all panes
    /// uniformly.
    frame_clear_color: wgpu::Color,
    /// Stable per-workspace render target. Unlike a swap-chain texture, its
    /// contents remain defined across frames and WebView2 compositor churn.
    frame_store: wgpu::Texture,
    frame_store_view: wgpu::TextureView,
    blit_bind_group_layout: wgpu::BindGroupLayout,
    blit_sampler: wgpu::Sampler,
    blit_pipeline: wgpu::RenderPipeline,
    blit_bind_group: wgpu::BindGroup,
    /// Per-frame transients. Populated by `begin_frame`, drained by
    /// `end_frame`. `record_pane` mutates the encoder via
    /// `begin_render_pass`. None outside the begin..end window.
    current_frame: Option<wgpu::SurfaceTexture>,
    current_surface_view: Option<wgpu::TextureView>,
    current_encoder: Option<wgpu::CommandEncoder>,
}

impl SurfaceHost {
    /// Construct a new host bound to `canvas`. Per-workspace model
    /// (2026-05-08 refactor): JS creates ONE SurfaceHost per workspace
    /// tab so each tab's canvas keeps its own swap chain. The browser's
    /// compositor preserves the inactive tab's last-painted pixels as
    /// long as the canvas DOM element stays mounted, giving instant
    /// (no-flash) workspace switches.
    ///
    /// The shared `GpuContext` (instance / device / queue / pipeline /
    /// atlas / rasterizer / sampler) stays a process-wide singleton —
    /// only the `Surface` + per-frame transients are per-workspace.
    /// Memory cost: ~14 MiB per workspace at typical resolution
    /// (2 swap-chain textures × BGRA × ~4 MP).
    ///
    /// Returns `Err` if the WebGPU adapter / device acquisition fails or
    /// `instance.create_surface` rejects the canvas. JS catches and falls
    /// back to per-pane Canvas2D (each pane gets its own DOM canvas).
    pub async fn init(canvas: HtmlCanvasElement) -> Result<Rc<RefCell<Self>>, String> {
        let ctx = GpuContext::get_or_init().await?;
        let surface = {
            let ctx_b = ctx.borrow();
            ctx_b
                .instance
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
                .map_err(|e| format!("SurfaceHost: create_surface failed: {e:?}"))?
        };
        // Seed config with size 1×1 — JS calls `resize(w, h, dpr)`
        // synchronously after `init` to apply the real dimensions, so
        // this is just a placeholder that satisfies wgpu's "must
        // configure before get_current_texture" rule.
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: CANVAS_FORMAT,
            width: 1,
            height: 1,
            present_mode: wgpu::PresentMode::Fifo,
            // PreMultiplied (not Auto) — on WebView2/Chromium, `Auto`
            // resolves to `Opaque`, which makes the compositor ignore
            // the swap-chain alpha entirely: idle frames (or zero-init
            // textures after WebView2 recycles the swap chain in idle
            // periods) display as RGB=(0,0,0) opaque black, regardless
            // of what the DOM parent stack looks like. Explicit
            // `PreMultiplied` makes transparent pixels actually
            // transparent at composite time, so splitter strips show
            // their SplitContainer DOM bg and idle/recycled regions
            // fall through to the canvas's CSS parents instead of
            // turning black.
            alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
            view_formats: vec![],
            // P1.1 (2026-05-19): lat=1 so `get_current_texture` deterministically
            // returns the just-presented frame N-1, not frame N-2. That makes
            // `LoadOp::Load` actually preserve last-frame content, which is
            // the entire point of the per-pane LoadOp::Load record path. With
            // the prior lat=2 the swap chain returned an N-2 texture whose
            // content could be anything (including the very first cleared
            // frame), forcing `requires_full_frame()` to be hard-coded `true`
            // and burning O(rows × cols) cell encodes per pane per tick. The
            // throughput cost of lat=1 (CPU may stall ~1 frame waiting for
            // GPU to release the buffer) is invisible for an idle terminal
            // — typical wgpu submit + present is well under 16.6 ms even on
            // an integrated GPU, and the saved encode cost dwarfs it under
            // load anyway.
            desired_maximum_frame_latency: 1,
        };
        {
            let ctx_b = ctx.borrow();
            surface.configure(&ctx_b.device, &config);
        }
        let (
            frame_store,
            frame_store_view,
            blit_bind_group_layout,
            blit_sampler,
            blit_pipeline,
            blit_bind_group,
        ) = {
            let ctx_b = ctx.borrow();
            let (frame_store, frame_store_view) = create_frame_target(&ctx_b.device, 1, 1);
            let (blit_bind_group_layout, blit_sampler, blit_pipeline, blit_bind_group) =
                create_blit_resources(&ctx_b.device, &frame_store_view);
            (
                frame_store,
                frame_store_view,
                blit_bind_group_layout,
                blit_sampler,
                blit_pipeline,
                blit_bind_group,
            )
        };

        Ok(Rc::new(RefCell::new(Self {
            ctx,
            surface,
            config,
            frame_clear_color: wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            frame_store,
            frame_store_view,
            blit_bind_group_layout,
            blit_sampler,
            blit_pipeline,
            blit_bind_group,
            current_frame: None,
            current_surface_view: None,
            current_encoder: None,
        })))
    }

    /// Resize the host canvas's swap chain. Called by JS in response to
    /// the host-parent `ResizeObserver` (window resize, sidebar toggle,
    /// DPR change). Idempotent: same `(width_css, height_css, dpr)` is
    /// short-circuited so spurious observer fires don't churn the
    /// surface.
    ///
    /// `width_css` / `height_css` are CSS pixels; backing-pixel size is
    /// `(width_css * dpr, height_css * dpr)`. JS is responsible for
    /// updating `canvas.width / canvas.height` in lockstep so the
    /// surface configure matches the HTML element's allocation.
    pub fn resize(&mut self, width_css: u32, height_css: u32, dpr: f32) {
        let backing_w = ((width_css as f32) * dpr).round().max(1.0) as u32;
        let backing_h = ((height_css as f32) * dpr).round().max(1.0) as u32;
        if self.config.width == backing_w && self.config.height == backing_h {
            return;
        }
        self.config.width = backing_w;
        self.config.height = backing_h;
        self.surface
            .configure(&self.ctx.borrow().device, &self.config);
        let (frame_store, frame_store_view) = {
            let ctx = self.ctx.borrow();
            create_frame_target(&ctx.device, backing_w, backing_h)
        };
        let blit_bind_group = {
            let ctx = self.ctx.borrow();
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ridge-host-frame-blit-bind-group"),
                layout: &self.blit_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&frame_store_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.blit_sampler),
                    },
                ],
            })
        };
        self.frame_store = frame_store;
        self.frame_store_view = frame_store_view;
        self.blit_bind_group = blit_bind_group;
    }

    /// Retained for the JS invalidation protocol. The frame store is seeded
    /// every frame, so no Rust-side clear flag is needed here.
    pub fn invalidate(&mut self) {
    }

    /// Current swap-chain backing-pixel width, used by JS to clamp
    /// per-pane scissor rects before forwarding them to the renderer.
    pub fn width(&self) -> u32 {
        self.config.width
    }

    /// Current swap-chain backing-pixel height — paired with `width`.
    pub fn height(&self) -> u32 {
        self.config.height
    }

    /// Begin one host frame: acquire a swap-chain texture and create the
    /// per-frame encoder. Subsequent `record_pane` calls open render
    /// passes against this encoder; `end_frame` finishes + submits +
    /// presents.
    ///
    /// Returns `false` on surface-lost / outdated — the caller (JS RAF
    /// loop) skips the rest of the frame and the next tick retries.
    /// `theme_bg` is the 4-byte RGBA seed color for the frame-store pass.
    pub fn begin_frame(&mut self, theme_bg: [u8; 4]) -> bool {
        if self.current_frame.is_some() {
            // Stale frame from a previous tick that never ended (likely
            // a JS bug). Drop the transients and start fresh — better
            // than panicking inside a swap-chain double-acquire.
            self.current_encoder = None;
            self.current_surface_view = None;
            self.current_frame = None;
        }
        self.frame_clear_color = rgba_to_wgpu_color(theme_bg);

        // The offscreen frame store is always seeded before pane passes.
        // Default shell cells intentionally use transparent backgrounds so
        // wallpaper can show through, which means a dirty-row-only pass
        // cannot erase glyphs from the previous frame. Full seeding plus
        // full pane re-encoding prevents retained-pixel ghosting while the
        // final swap-chain blit remains a single atomic presentation.

        // Reset the global frame-written mask so all atlas layers are
        // available for writing in this new frame, then apply any deferred
        // atlas invalidation (resize/reflow/font change). Doing the clear HERE
        // — at the frame boundary, with no pane having cited a layer yet — is
        // what makes resetting `next_free_layer` safe; doing it mid-frame
        // (where `invalidate_atlas` is actually called from) clobbers sibling
        // panes' recorded draws (the switch garble) or starves the fresh-layer
        // pointer (the flicker).
        {
            let mut ctx = self.ctx.borrow_mut();
            ctx.reset_frame_written();
            ctx.apply_pending_invalidate();
        }

        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                // WebView2 can invalidate the swap chain without emitting a
                // ResizeObserver event. Reconfigure immediately so the next
                // RAF can acquire a fresh texture instead of staying blank.
                self.surface
                    .configure(&self.ctx.borrow().device, &self.config);
                return false;
            }
            Err(_) => return false,
        };
        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.ctx
                .borrow()
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("ridge-host-frame-encoder"),
                });

        // Perform a single global seed pass for every frame.
        // When a wallpaper is active, draw the full-screen quad instead of a
        // plain colour clear so the wallpaper is composited beneath every pane.
        // The wallpaper fragment always outputs alpha=1, so it fully replaces
        // Any stale pixels are replaced before pane passes run.
        // §wallpaper-fix: 壁纸激活时**每帧无条件**画全屏 quad（顶替 clear）；
        // 无壁纸时维持原行为，仅在 needs_initial_clear 帧做普通 clear。把壁纸
        // 绘制 gate 在 needs_initial_clear 之内会让非首帧（光标闪烁等局部重绘）
        // 不重画壁纸，透明默认单元回放时透出双缓冲陈旧像素 → 鬼影
        // （见 plan「核心正确性规则」）。
        {
            let ctx = self.ctx.borrow();
            if ctx.has_wallpaper() {
                // ── Wallpaper path ──────────────────────────────────────────
                // 1. Compute cover-UV so the image fills the surface at equal
                //    aspect ratio, cropped and centred.
                let (surf_w, surf_h) = (self.config.width, self.config.height);
                let wp = ctx.wallpaper.as_ref().unwrap();
                let uv = cover_uv_transform(surf_w, surf_h, wp.img_w, wp.img_h);

                // 2. Build the 32-byte uniform buffer (std140):
                //    bytes  0.. 8  uv_scale : vec2<f32>
                //    bytes  8..16  uv_offset: vec2<f32>
                //    bytes 16..28  bg_rgb   : vec3<f32>  (12 bytes)
                //    bytes 28..32  opacity  : f32        (4-byte std140 pad is satisfied)
                let mut bytes = [0u8; WALLPAPER_UNIFORM_SIZE as usize];
                bytes[0..4].copy_from_slice(&uv.scale[0].to_le_bytes());
                bytes[4..8].copy_from_slice(&uv.scale[1].to_le_bytes());
                bytes[8..12].copy_from_slice(&uv.offset[0].to_le_bytes());
                bytes[12..16].copy_from_slice(&uv.offset[1].to_le_bytes());
                let bg = self.frame_clear_color;
                bytes[16..20].copy_from_slice(&(bg.r as f32).to_le_bytes());
                bytes[20..24].copy_from_slice(&(bg.g as f32).to_le_bytes());
                bytes[24..28].copy_from_slice(&(bg.b as f32).to_le_bytes());
                bytes[28..32].copy_from_slice(&ctx.wallpaper_opacity.to_le_bytes());
                ctx.queue.write_buffer(&ctx.wallpaper_uniform, 0, &bytes);

                // 3. Render full-screen TriangleStrip (4 vertices, no VB).
                let bg_group = ctx.wallpaper_bind_group.as_ref().unwrap();
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ridge-host-wallpaper-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.frame_store_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            // quad 不透明铺满整屏，等效 clear → Load 即可，省一次 clear。
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&ctx.wallpaper_pipeline);
                pass.set_bind_group(0, bg_group, &[]);
                pass.draw(0..4, 0..1);
                // pass dropped → render pass ends
            } else {
                // ── Plain colour clear path ─────────────────────────────────
                let mut _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ridge-host-clear-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.frame_store_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(self.frame_clear_color),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                // pass dropped here
            }
        }
        self.current_frame = Some(frame);
        self.current_surface_view = Some(surface_view);
        self.current_encoder = Some(encoder);
        true
    }

    /// Open a render pass for one pane, set its viewport + scissor + the
    /// shared cell pipeline, then hand the pass to the closure for bind
    /// group / vertex buffer / draw recording. The pass is dropped at
    /// the end of the closure so the encoder can accept the next pane's
    /// pass.
    ///
    /// Always uses `LoadOp::Load` — the full-surface clear was already
    /// issued by [`begin_frame`] as a dedicated render pass.
    /// Empty / out-of-bounds scissors are no-ops — wgpu validation
    /// rejects zero-extent viewports.
    pub fn record_pane<F>(
        &mut self,
        scissor: ScissorRect,
        pipeline: &wgpu::RenderPipeline,
        record: F,
    ) where
        F: FnOnce(&mut wgpu::RenderPass<'_>),
    {
        if scissor.is_empty() {
            println!("[ridge-term] Scissor empty, clipping: {:?}", scissor);
            return;
        }
        // Clamp scissor to swap-chain dimensions to avoid wgpu validation errors
        let x = scissor.x.min(self.config.width);
        let y = scissor.y.min(self.config.height);
        let w = scissor.w.min(self.config.width - x);
        let h = scissor.h.min(self.config.height - y);

        if w == 0 || h == 0 {
            return;
        }

        let load = wgpu::LoadOp::Load;

        let encoder = match self.current_encoder.as_mut() {
            Some(e) => e,
            None => return,
        };

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ridge-host-pane-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.frame_store_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            // Map the pane's [-1, 1] NDC range to its rect on the host
            // canvas. The pane's vertex shader divides cell_xy by
            // frame_uniform.viewport (= scissor.w × scissor.h), so this
            // is the correct NDC → device-pixel mapping.
            pass.set_viewport(
                scissor.x as f32,
                scissor.y as f32,
                w as f32,
                h as f32,
                0.0,
                1.0,
            );
            pass.set_scissor_rect(scissor.x, scissor.y, w, h);
            pass.set_pipeline(pipeline);
            record(&mut pass);
        }
    }

    /// Upload a new wallpaper image and enable the wallpaper path in
    /// `begin_frame`. `rgba` must be packed RGBA8, row-major. The call
    /// also triggers `invalidate` so the next frame's seed pass redraws
    /// the wallpaper immediately.
    pub fn set_wallpaper(&mut self, rgba: &[u8], w: u32, h: u32, opacity: f32) {
        self.ctx.borrow_mut().set_wallpaper(rgba, w, h, opacity);
        self.invalidate();
    }

    /// Remove the active wallpaper and fall back to the plain colour
    /// `LoadOp::Clear` path. Also triggers `invalidate`.
    pub fn clear_wallpaper(&mut self) {
        self.ctx.borrow_mut().clear_wallpaper();
        self.invalidate();
    }

    /// Finish the encoder, submit, present. Resets transients so the
    /// next `begin_frame` starts cleanly. No-op if `begin_frame` was
    /// never called or already returned `false` (surface lost).
    pub fn end_frame(&mut self) {
        let encoder = match self.current_encoder.take() {
            Some(e) => e,
            None => return,
        };
        let frame = match self.current_frame.take() {
            Some(f) => f,
            None => return,
        };
        let surface_view = match self.current_surface_view.take() {
            Some(v) => v,
            None => return,
        };
        {
            let mut encoder = encoder;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ridge-host-frame-blit-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.frame_clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &self.blit_bind_group, &[]);
            pass.draw(0..4, 0..1);
            drop(pass);
            self.ctx.borrow().queue.submit(Some(encoder.finish()));
        }
        frame.present();
        // If no pane drew this frame, the encoder still contains the seed
        // pass, so the presented frame cannot expose stale frame-store data.
    }
}
