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
//! ## Browser GPU selection
//!
//! The first host canvas supplies the compatible surface required by WebGL2.
//! `wgpu` probes WebGPU first and removes it when no real adapter exists, then
//! uses WebGL2 through the same renderer API. Surface format, alpha mode, and
//! device resolution limits come from that adapter and surface.

#![cfg(all(target_arch = "wasm32", feature = "webgpu"))]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use web_sys::HtmlCanvasElement;

use super::glyph_atlas::{pick_evictable_layer, GlyphAtlas, GlyphEntry, GlyphKey};
use super::glyph_rasterizer::{GlyphRasterizer, RasterizedGlyph};

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

/// Keep atlas rasterisation at native device density. The browser already
/// antialiases glyphs; rendering at 2× and linearly downsampling softened
/// fixed-width text, especially on fractional DPR displays (for example
/// 1.25). Keep this switch explicit so a future emoji-only supersampling
/// path can be measured independently.
pub const ATLAS_SUPERSAMPLE: u32 = 1;

/// Prefer linear BGRA for WebGPU and linear RGBA for WebGL2. Keeping a
/// capabilities fallback lets future browser formats fail at pipeline
/// validation rather than at adapter selection.
fn select_surface_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    [
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
    ]
    .into_iter()
    .find(|candidate| formats.contains(candidate))
    .or_else(|| formats.first().copied())
}

/// std140 size of `WallpaperUniform`: vec2(8) + vec2(8) + vec3-padded-to-vec4(16) = 32 bytes.
pub const WALLPAPER_UNIFORM_SIZE: u64 = 32;

/// Uploaded wallpaper image GPU resources (texture + view + original pixel dimensions).
pub struct WallpaperTex {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub img_w: u32,
    pub img_h: u32,
}

/// CPU-side copy of recently rasterized glyph bitmaps. The GPU atlas is an
/// LRU of texture layers, so a glyph can be evicted while its bitmap is still
/// useful. Keeping a bounded copy avoids another synchronous Canvas2D
/// `get_image_data` readback when that glyph returns under high pane churn.
const RASTER_CACHE_MAX_BYTES: usize = 8 * 1024 * 1024;
const RASTER_CACHE_MAX_ENTRIES: usize = 2048;

struct RasterCacheEntry {
    glyph: Rc<RasterizedGlyph>,
    last_used: u64,
    bytes: usize,
}

#[derive(Default)]
struct RasterizedGlyphCache {
    entries: HashMap<GlyphKey, RasterCacheEntry>,
    clock: u64,
    bytes: usize,
}

impl RasterizedGlyphCache {
    fn next_stamp(&mut self) -> u64 {
        if self.clock == u64::MAX {
            for entry in self.entries.values_mut() {
                entry.last_used = 0;
            }
            self.clock = 0;
        }
        self.clock += 1;
        self.clock
    }

    fn get(&mut self, key: &GlyphKey) -> Option<Rc<RasterizedGlyph>> {
        let stamp = self.next_stamp();
        let entry = self.entries.get_mut(key)?;
        entry.last_used = stamp;
        Some(entry.glyph.clone())
    }

    fn insert(&mut self, key: GlyphKey, glyph: Rc<RasterizedGlyph>) {
        let bytes = glyph.rgba.len();
        if bytes > RASTER_CACHE_MAX_BYTES {
            return;
        }
        let stamp = self.next_stamp();
        if let Some(existing) = self.entries.get_mut(&key) {
            self.bytes = self.bytes.saturating_sub(existing.bytes);
            existing.glyph = glyph;
            existing.bytes = bytes;
            existing.last_used = stamp;
            self.bytes = self.bytes.saturating_add(bytes);
            return;
        }

        while self.entries.len() >= RASTER_CACHE_MAX_ENTRIES
            || self.bytes.saturating_add(bytes) > RASTER_CACHE_MAX_BYTES
        {
            let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some(oldest) = self.entries.remove(&oldest_key) {
                self.bytes = self.bytes.saturating_sub(oldest.bytes);
            }
        }

        self.bytes = self.bytes.saturating_add(bytes);
        self.entries.insert(
            key,
            RasterCacheEntry {
                glyph,
                last_used: stamp,
                bytes,
            },
        );
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.clock = 0;
        self.bytes = 0;
    }
}

/// Per-process shared GPU resources. One instance for all panes.
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_format: wgpu::TextureFormat,
    pub surface_alpha_mode: wgpu::CompositeAlphaMode,
    pub backend_name: &'static str,

    pub cell_shader: wgpu::ShaderModule,
    pub cell_bind_group_layout: wgpu::BindGroupLayout,
    pub cell_pipeline: wgpu::RenderPipeline,
    pub sampler: wgpu::Sampler,

    // ── 壁纸资源 ─────────────────────────────────────────────────────
    /// 当前壁纸纹理（含原始像素尺寸）。`None` = 无壁纸。
    pub wallpaper: Option<WallpaperTex>,
    /// 壁纸不透明度 [0.0, 1.0]，由 `set_wallpaper` 写入，
    /// `begin_frame` 读取后填入 uniform 并更新 GPU buffer。
    pub wallpaper_opacity: f32,
    /// 全屏 quad 渲染管线（TriangleStrip，无顶点 buffer）。
    /// Task 3 (`surface_host.rs::begin_frame`) 从另一模块直接访问，故 `pub`。
    pub wallpaper_pipeline: wgpu::RenderPipeline,
    wallpaper_sampler: wgpu::Sampler,
    /// 壁纸 uniform buffer（WALLPAPER_UNIFORM_SIZE 字节）。
    pub wallpaper_uniform: wgpu::Buffer,
    wallpaper_bgl: wgpu::BindGroupLayout,
    /// 壁纸 bind group（含 uniform/texture/sampler）。上传图片后重建。
    pub wallpaper_bind_group: Option<wgpu::BindGroup>,

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
    raster_cache: RasterizedGlyphCache,
    pub slot_w: u32,
    pub slot_h: u32,
    /// Bumped every time `atlas_texture` / `atlas_view` is recreated.
    /// Per-pane backends compare their last-seen value at frame start;
    /// mismatch → rebuild bind group against the new view.
    pub atlas_generation: u64,
    /// Running diagnostic count of layers evicted and reused by new glyphs.
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

    /// §atlas-race detector (2026-06-22): per-layer "a pane already RECORDED
    /// a draw citing this layer THIS frame" mask, same length as
    /// `atlas_layers`. Unlike `frame_written` (set eagerly when a layer is
    /// admitted/cited), this is set AFTER a pane hands its draw to the host
    /// encoder (`end_frame`), from the layers its
    /// instance buffer actually references. It is the GROUND TRUTH of "this
    /// slot's pixels are now load-bearing for an unsubmitted draw". If
    /// `rasterize_and_admit` overwrites a layer whose `frame_committed` is
    /// set, that recorded draw samples the new glyph at submit time — the
    /// exact cross-pane switch-workspace garble. `frame_written` SHOULD make
    /// this impossible; a hit pinpoints a citing path that skipped it.
    pub frame_committed: Vec<bool>,
    /// §atlas-race detector: running count of overwrite-after-cite events.
    /// Surfaced to JS via `atlas_overwrite_after_cite_count` /
    /// `atlasOverwriteAfterCiteCount` for CDP/release forensics.
    pub atlas_overwrite_after_cite: u64,
    /// §atlas-race detector: remaining console-log budget. The counter is
    /// unbounded but logging stops after this many detections so a churn
    /// storm can't flood devtools.
    pub atlas_cite_log_budget: u32,

    /// §switch-garble fix (2026-06-24): `invalidate_atlas` is called from
    /// `Renderer::invalidate_all` on EVERY resize/reflow — often mid-host-frame
    /// (one pane reflows while siblings have already recorded draws against the
    /// shared atlas). Clearing the map + resetting `next_free_layer` right then,
    /// on the REUSED texture, either clobbers those siblings' cited layers (the
    /// switch garble) or — if we skip written slots — exhausts the fresh-layer
    /// pointer under density (random missing glyphs that flicker). So
    /// `invalidate_atlas` only RAISES this flag; the actual clear is applied at
    /// the next `reset_frame_written` (host frame boundary), where no pane has
    /// cited anything yet and resetting `next_free_layer` is always safe.
    pub pending_invalidate: bool,

    pub font_family: String,
    pub font_size_px: f32,
}

thread_local! {
    /// Process-wide singleton. `None` until the first
    /// `GpuContext::get_or_init_for_canvas` call succeeds; cached thereafter.
    /// Failure is *not* cached — each call re-attempts so a transient
    /// adapter miss does not permanently lock the session out of WebGPU.
    static SHARED_GPU: RefCell<Option<Rc<RefCell<GpuContext>>>> = const { RefCell::new(None) };
}

/// §atlas-race detector: read the process-wide overwrite-after-cite count
/// from the shared GPU context. Returns 0 before the context initializes or
/// before the first frame. Surfaced to JS via `lib.rs::atlasOverwriteAfterCiteCount`.
pub fn atlas_overwrite_after_cite_count() -> u64 {
    SHARED_GPU.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|rc| rc.borrow().atlas_overwrite_after_cite)
            .unwrap_or(0)
    })
}

/// §stale-replay detector: read the process-wide count of cached replays
/// aborted because a cited atlas layer was repurposed since caching (the
/// cross-frame switch-workspace garble). 0 before the GPU context inits.
impl GpuContext {
    /// Lazily acquire the shared GPU context. First call performs the
    /// full browser GPU bootstrap (instance + adapter + device + pipeline +
    /// atlas); subsequent calls return the cached `Rc`.
    ///
    /// Returns `Err` on adapter / device acquisition failure so the
    /// caller (`WebGpuBackend::new`, eventually `RenderHandle
    /// ::newWithWebgpuFirst`) can report an explicit initialization error.
    /// Failure is not memoized — a flaky adapter on call N can succeed on
    /// call N+1.
    pub async fn get_or_init_for_canvas(
        canvas: HtmlCanvasElement,
    ) -> Result<(Rc<RefCell<Self>>, wgpu::Surface<'static>), String> {
        if let Some(rc) = SHARED_GPU.with(|cell| cell.borrow().clone()) {
            let surface = rc
                .borrow()
                .instance
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
                .map_err(|e| format!("GpuContext: create_surface failed: {e:?}"))?;
            return Ok((rc, surface));
        }

        let instance = wgpu::util::new_instance_with_webgpu_detection(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            ..Default::default()
        })
        .await;
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| format!("GpuContext: create_surface failed: {e:?}"))?;
        let ctx = Self::new(instance, &surface).await?;
        let rc = Rc::new(RefCell::new(ctx));
        SHARED_GPU.with(|cell| *cell.borrow_mut() = Some(rc.clone()));
        Ok((rc, surface))
    }

    /// Bootstrap. Creates instance + adapter + device, then builds the
    /// shader / pipeline / atlas / rasterizer / sampler.
    async fn new(
        instance: wgpu::Instance,
        compatible_surface: &wgpu::Surface<'_>,
    ) -> Result<Self, String> {
        // WebGL2 requires adapter selection against the canvas surface;
        // WebGPU also benefits from rejecting incompatible adapters here.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(compatible_surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| "GpuContext: no WebGPU or WebGL2 adapter available".to_string())?;

        let adapter_info = adapter.get_info();
        let backend_name = match adapter_info.backend {
            wgpu::Backend::BrowserWebGpu => "WebGPU",
            wgpu::Backend::Gl => "WebGL2",
            _ => "GPU",
        };
        let capabilities = compatible_surface.get_capabilities(&adapter);
        let surface_format = select_surface_format(&capabilities.formats)
            .ok_or_else(|| "GpuContext: surface exposes no texture format".to_string())?;
        let surface_alpha_mode = if capabilities
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else {
            capabilities
                .alpha_modes
                .first()
                .copied()
                .ok_or_else(|| "GpuContext: surface exposes no alpha mode".to_string())?
        };

        // Pick texture-array depth before requesting the device — wgpu
        // only honors `max_texture_array_layers` up to whatever we
        // declare in `required_limits`. Adapters typically advertise
        // 2048 (desktop) or 256 (WebGPU MVP floor); clamp into
        // [`ATLAS_LAYERS_MIN`, `ATLAS_LAYERS_MAX`] so memory stays
        // bounded while giving Claude-style TUIs (CJK + box-drawing
        // + spinner glyphs) enough cache headroom to avoid LRU thrash.
        let adapter_limits = adapter.limits();
        let atlas_layers: u32 = adapter_limits
            .max_texture_array_layers
            .clamp(ATLAS_LAYERS_MIN, ATLAS_LAYERS_MAX);
        let mut required_limits = if adapter_info.backend == wgpu::Backend::Gl {
            wgpu::Limits::downlevel_webgl2_defaults()
        } else {
            wgpu::Limits::downlevel_defaults()
        }
        .using_resolution(adapter_limits);
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
                    format: surface_format,
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
            // Glyphs are rasterized at the current device scale and their
            // quads preserve that device-pixel extent. Linear filtering would
            // blend neighbouring transparent texels whenever the pane origin
            // or DPR lands between pixels, producing a soft fringe and visible
            // seams between adjacent box-drawing glyphs. Sample the native
            // bitmap exactly; wallpaper scaling keeps its separate sampler.
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // ── 壁纸管线（全屏 quad）──────────────────────────────────
        let wallpaper_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ridge-wallpaper-shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shaders/wallpaper.wgsl"
            ))),
        });
        let wallpaper_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ridge-wallpaper-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
                        view_dimension: wgpu::TextureViewDimension::D2,
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
        let wallpaper_pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ridge-wallpaper-pipeline-layout"),
            bind_group_layouts: &[&wallpaper_bgl],
            push_constant_ranges: &[],
        });
        let wallpaper_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ridge-wallpaper-pipeline"),
            layout: Some(&wallpaper_pl_layout),
            vertex: wgpu::VertexState {
                module: &wallpaper_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[], // 全屏 quad 由 vertex_index 生成，无顶点 buffer
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
                module: &wallpaper_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let wallpaper_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ridge-wallpaper-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let wallpaper_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ridge-wallpaper-uniform"),
            size: WALLPAPER_UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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
            // gamma-decode the DOM canvas's sRGB-byte glyph pixels
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
            surface_format,
            surface_alpha_mode,
            backend_name,
            cell_shader,
            cell_bind_group_layout,
            cell_pipeline,
            sampler,
            wallpaper: None,
            wallpaper_opacity: 1.0,
            wallpaper_pipeline,
            wallpaper_sampler,
            wallpaper_uniform,
            wallpaper_bgl,
            wallpaper_bind_group: None,
            atlas: GlyphAtlas::new(atlas_capacity),
            atlas_texture,
            atlas_view,
            next_free_layer: ATLAS_RESERVED_LAYERS,
            atlas_layers,
            rasterizer,
            raster_cache: RasterizedGlyphCache::default(),
            slot_w,
            slot_h,
            atlas_generation: 0,
            atlas_eviction_count: 0,
            frame_written: vec![false; atlas_layers as usize],
            frame_committed: vec![false; atlas_layers as usize],
            atlas_overwrite_after_cite: 0,
            atlas_cite_log_budget: 64,
            pending_invalidate: false,
            font_family: String::from("monospace"),
            font_size_px: 15.0,
        })
    }

    /// Compute the device-pixel atlas slot size required for the given
    /// (cell_w, cell_h, dpr). Wide CJK cells need ≥ `cell_w × dpr × 2`
    /// device pixels horizontally so the rasterizer's DOM canvas
    /// holds the full advance without clipping. `slot_w` is rounded up
    /// to a power of two so `bytes_per_row = slot_w × 4` always
    /// satisfies wgpu's 256-byte alignment. Vertical adds 25% safety
    /// for descenders / italic overhang / stacked combining marks.
    pub fn slot_dims_for(cell_w_css: f32, cell_h_css: f32, dpr: f32) -> (u32, u32) {
        // Keep slot dimensions tied to the rasteriser's density. The
        // current native-DPR setting keeps fixed-width glyphs 1:1; the
        // multiplier remains explicit for any future measured upscale.
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
    // ── 壁纸 API ─────────────────────────────────────────────────────────

    /// 上传一张 RGBA 图像作为当前 workspace 壁纸。
    /// 若 `w == 0 || h == 0` 则等同 `clear_wallpaper`。
    /// `opacity` 被 clamp 到 [0, 1]。
    pub fn set_wallpaper(&mut self, rgba: &[u8], w: u32, h: u32, opacity: f32) {
        if w == 0 || h == 0 {
            self.clear_wallpaper();
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ridge-wallpaper-tex"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let (packed, bytes_per_row) = super::wallpaper::pack_rows_to_alignment(rgba, w, h);
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &packed,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ridge-wallpaper-bg"),
            layout: &self.wallpaper_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.wallpaper_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.wallpaper_sampler),
                },
            ],
        });
        self.wallpaper = Some(WallpaperTex {
            texture,
            view,
            img_w: w,
            img_h: h,
        });
        self.wallpaper_opacity = opacity.clamp(0.0, 1.0);
        self.wallpaper_bind_group = Some(bind_group);
    }

    /// 清除壁纸，回退到纯色 clear pass。
    pub fn clear_wallpaper(&mut self) {
        self.wallpaper = None;
        self.wallpaper_bind_group = None;
    }

    /// 当前是否有有效壁纸纹理 + bind group。
    pub fn has_wallpaper(&self) -> bool {
        self.wallpaper.is_some() && self.wallpaper_bind_group.is_some()
    }

    /// Reset the per-frame written mask. Called at the start of every
    /// frame from `SurfaceHost::begin_frame` so each frame starts with
    /// all layers available for writing.
    pub fn reset_frame_written(&mut self) {
        for w in &mut self.frame_written {
            *w = false;
        }
        // §atlas-race detector: the committed-citation mask is per-frame too.
        for c in &mut self.frame_committed {
            *c = false;
        }
    }

    /// §atlas-race detector: mark `layer` as cited by a draw that has already
    /// been RECORDED into the host encoder this frame. Called from the
    /// per-pane `end_frame` right after `queue_pane`.
    /// See [`Self::frame_committed`].
    pub fn mark_committed(&mut self, layer: u16) {
        let idx = layer as usize;
        if idx < self.frame_committed.len() {
            self.frame_committed[idx] = true;
        }
    }

    pub fn rebuild_atlas(&mut self) -> Result<(), String> {
        self.atlas.clear();
        self.raster_cache.clear();
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

        // Rasterizer's DOM canvas dimensions must match the slot
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
        // DEFER, don't clear. `Renderer::invalidate_all` calls this on every
        // resize/reflow, frequently MID-host-frame (one pane reflows while
        // sibling panes have already admitted glyphs + recorded draws against
        // the shared, REUSED atlas texture). Clearing the map and resetting
        // `next_free_layer` right here would re-hand-out the siblings' cited
        // layers and `write_texture` would clobber the pixels their recorded
        // draws sample at submit — the switch-workspace garble; skipping
        // written slots instead starves the fresh-layer pointer and drops
        // glyphs (the flicker). Both are avoided by applying the clear at the
        // next frame boundary (`apply_pending_invalidate`), where
        // `reset_frame_written` has just cleared every cited mark. See
        // `pending_invalidate`. We do NOT bump `atlas_generation` here — that
        // happens at apply time so panes see it on the same frame the atlas is
        // actually cleared.
        self.pending_invalidate = true;
        self.raster_cache.clear();
    }

    /// Apply a deferred [`Self::invalidate_atlas`] at the host frame boundary.
    /// Called from `SurfaceHost::begin_frame` immediately after
    /// [`Self::reset_frame_written`], so the per-frame cited mask is empty and
    /// resetting `next_free_layer` cannot clobber any pane's recorded draw.
    pub fn apply_pending_invalidate(&mut self) {
        if !self.pending_invalidate {
            return;
        }
        self.pending_invalidate = false;
        self.atlas.clear();
        self.raster_cache.clear();
        self.next_free_layer = ATLAS_RESERVED_LAYERS;
        // Bump generation HERE (not in `invalidate_atlas`) so panes observe the
        // change on the SAME frame the map is actually cleared: each pane's
        // `begin_frame` rebuilds its bind group + drops `cached_n_cells`,
        // forcing a full re-admit into the cleared atlas. Bumping earlier would
        // let panes consume the generation before the clear, then replay cached
        // buffers citing now-reused layers.
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
        // Pass the explicit atlas density to the rasteriser. At the
        // current native-DPR setting this avoids an extra filtered
        // downsample; the conversion below keeps future scaling local.
        let ss = ATLAS_SUPERSAMPLE as f32;
        let glyph = if let Some(glyph) = self.raster_cache.get(&key) {
            glyph
        } else {
            let glyph = Rc::new(self.rasterizer.rasterize(
                &self.font_family,
                self.font_size_px,
                dpr * ss,
                style_flags,
                glyph_text,
            )?);
            self.raster_cache.insert(key, glyph.clone());
            glyph
        };

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

        // §atlas-race detector (2026-06-22): we are about to overwrite
        // `layer`'s pixels. If any pane already RECORDED a draw citing this
        // layer this frame (`frame_committed`), that draw will sample the new
        // glyph at submit time → the cross-pane switch-workspace garble. This
        // is supposed to be impossible: `pick_evictable_layer` skips
        // `frame_written`, and the fresh `next_free_layer` path never reuses a
        // live layer. A hit therefore localises the residual hole — log the
        // glyph + whether `frame_written` was (wrongly) clear for this layer.
        if self
            .frame_committed
            .get(layer as usize)
            .copied()
            .unwrap_or(false)
        {
            self.atlas_overwrite_after_cite = self.atlas_overwrite_after_cite.wrapping_add(1);
            if self.atlas_cite_log_budget > 0 {
                self.atlas_cite_log_budget -= 1;
                let was_written = self
                    .frame_written
                    .get(layer as usize)
                    .copied()
                    .unwrap_or(false);
                web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&format!(
                    "[ridge-term][atlas-race] OVERWRITE-AFTER-CITE layer={} new_glyph={:?} \
                     glyph_id=0x{:08x} frame_written={} evict_count={} total={} \
                     (a recorded draw will sample the wrong glyph this frame)",
                    layer,
                    glyph_text,
                    key.glyph_id,
                    was_written,
                    self.atlas_eviction_count,
                    self.atlas_overwrite_after_cite,
                )));
            }
        }

        // Glyph bitmaps are tightly packed by the rasterizer. Pad each row
        // only to wgpu's copy alignment instead of uploading the entire slot.
        let upload_width = u32::from(glyph.width.max(1));
        let upload_height = u32::from(glyph.height.max(1));
        let (upload, bytes_per_row) =
            super::wallpaper::pack_rows_to_alignment(&glyph.rgba, upload_width, upload_height);
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
            &upload,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(upload_height),
            },
            wgpu::Extent3d {
                width: upload_width,
                height: upload_height,
                depth_or_array_layers: 1,
            },
        );

        // UV edges, not texel centres: the native-size quad maps each output
        // device pixel to one source texel under the nearest sampler.
        let u1 = glyph.width as f32 / self.slot_w as f32;
        let v1 = glyph.height as f32 / self.slot_h as f32;
        // `glyph.width / glyph.height` are atlas bitmap pixels. The
        // renderer sizes quads in logical device pixels, so divide by
        // the explicit atlas density here. UV ratios stay unchanged.
        let logical_px_w = ((glyph.width as u32) / ATLAS_SUPERSAMPLE).max(1) as u16;
        let logical_px_h = ((glyph.height as u32) / ATLAS_SUPERSAMPLE).max(1) as u16;
        let entry = GlyphEntry {
            layer: layer as u16,
            uv: [0.0, 0.0, u1, v1],
            advance: glyph.advance,
            // Rasterizer metrics are at the upload density; CellInstance
            // coordinates are native device pixels, so remove only the
            // explicit atlas supersample factor.
            ascent_offset: glyph.ascent_offset / ss,
            left_offset: glyph.left_offset / ss,
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

    // GpuContext construction requires a live browser GPU adapter — not
    // available in `cargo test --lib` (host target). These tests cover
    // pure logic that doesn't need the GPU: the slot-dim heuristic and
    // the pin-aware eviction walk. Browser smoke (plan §Verification)
    // covers the GPU-bearing paths, including atlas-generation
    // propagation across panes.

    fn slot_dims_for_pub(cell_w_css: f32, cell_h_css: f32, dpr: f32) -> (u32, u32) {
        // Mirrors the live `slot_dims_for` impl including the atlas scale
        // and the §B.10 3.0× wide-headroom factor so tests pin the actual
        // formula, not a stale copy.
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
        // 8×16 CSS px, DPR 1 stays under both atlas floors.
        let (w, h) = slot_dims_for_pub(8.0, 16.0, 1.0);
        assert_eq!(w, super::ATLAS_SLOT_W_FLOOR);
        assert_eq!(h, super::ATLAS_SLOT_H_FLOOR);
    }

    #[test]
    fn slot_dims_grow_for_large_font_at_high_dpr() {
        // 24 CSS px font at DPR 2 → cell_w_dev = 48,
        // wide_w = 48 × 3 = 144. Next power-of-two ≥ 144 is 256.
        // Vertical: row_h = 48, + 25% = 60 — 96 floor remains active.
        let (w, h) = slot_dims_for_pub(24.0, 24.0, 2.0);
        assert_eq!(w, 256);
        assert_eq!(h, 96);
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
        // 33 px wide cell × DPR 1 → wide_w = 33 × 3 = 99
        // → next power-of-two = 128.
        let (w, _) = slot_dims_for_pub(33.0, 16.0, 1.0);
        assert_eq!(w, 128);
    }

    #[test]
    fn slot_dims_grows_height_when_row_exceeds_floor() {
        // 100 css px row × DPR 2 → row_h_dev = 200 →
        // 200 + 50 = 250 wins over the 96 floor.
        let (_, h) = slot_dims_for_pub(8.0, 100.0, 2.0);
        assert_eq!(h, 250);
    }

    fn make_key(id: u32) -> GlyphKey {
        GlyphKey::new(0xdeadbeef, 1500, id, 0, 1.0)
    }

    fn make_entry(layer: u16) -> GlyphEntry {
        GlyphEntry {
            layer,
            uv: [0.0, 0.0, 1.0, 1.0],
            advance: 8.0,
            ascent_offset: 12.0,
            left_offset: 0.0,
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
