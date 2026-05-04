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

/// WebGPU backend — round 3 §4.1 first slice. Holds the device, queue,
/// surface, and the current swap-chain configuration. Glyph atlas + per-
/// frame buffers will land in §4.1.b once cosmic-text / fontdue is wired.
pub struct WebGpuBackend {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    atlas: GlyphAtlas,
    metrics: FrameMetrics,
    theme: Theme,
}

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

        Ok(Self {
            surface,
            device,
            queue,
            config,
            atlas: GlyphAtlas::new(1024),
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
