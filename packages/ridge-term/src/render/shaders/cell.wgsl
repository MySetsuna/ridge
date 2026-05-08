// Ridge terminal cell shader — Round 3 §4.1.c.
//
// Pipeline: textured-quad per cell. The vertex stage emits a single
// quad (4 vertices, TriangleStrip) per instance, positioning it at
// the cell's pixel coordinates and converting to NDC. The fragment
// stage samples the glyph atlas (texture_2d_array — one layer per
// rasterized glyph) and composites the cell's fg/bg colors using
// the alpha channel as glyph coverage.
//
// Why this shape:
//   - One pipeline handles all cells (background-only, glyph-bearing,
//     and bold/italic styles all flow through the same shader; style
//     just changes the font CSS used at rasterization time).
//   - Texture array (vs single 2D atlas with a UV grid) keeps glyph
//     packing trivial: each rasterized glyph gets its own layer with
//     the full slot rectangle. Future iteration can compact via
//     bin-packing if memory pressure justifies the complexity.
//   - Two-mode glyph rasterization in glyph_rasterizer.rs:
//     * Monochrome glyphs (ASCII / CJK / outline emoji) — painted
//       in pure white #ffffff, so RGB is always (1,1,1) and alpha
//       carries coverage. Fragment tints with fg.rgb at composite
//       time — load-bearing for SGR palette + 24-bit truecolor.
//     * Color emoji (COLR / CPAL / sbix / SVG fonts) — the browser
//       ignores fillStyle and stamps the font's native palette into
//       RGB. Fragment detects this per-pixel and passes RGB through
//       unchanged so single codepoint + ZWJ composite emoji stay
//       multicolor on WebGPU.
//     The two paths share one pipeline, one atlas format, one cache
//     key — the discriminator is per-pixel RGB inspection in fs_main.
//
// Per-instance attributes (loaded from the per-cell instance buffer):
//   @location(0) cell_xy     vec2<f32>  pixel position of cell top-left
//   @location(1) cell_size   vec2<f32>  pixel width × height (width-2 cells use 2× cell_w)
//   @location(2) atlas_uv    vec4<f32>  (u0, v0, u1, v1) within the atlas slot
//   @location(3) atlas_layer u32        texture-array layer index
//   @location(4) fg_rgba     vec4<f32>  cell foreground color, premult or straight (caller chooses)
//   @location(5) bg_rgba     vec4<f32>  cell background color
//
// Frame uniform:
//   @binding(0) viewport vec2<f32>      surface size in pixels (post-DPR)

struct InstanceIn {
    @location(0) cell_xy: vec2<f32>,
    @location(1) cell_size: vec2<f32>,
    @location(2) atlas_uv: vec4<f32>,
    @location(3) atlas_layer: u32,
    @location(4) fg_rgba: vec4<f32>,
    @location(5) bg_rgba: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // u32 in vertex outputs requires `@interpolate(flat)` and a
    // separate path; we pass via f32 and cast back in the fragment
    // stage. Layer indices are small (< 4096) so the f32 round-trip
    // is exact.
    @location(1) atlas_layer: f32,
    @location(2) fg: vec4<f32>,
    @location(3) bg: vec4<f32>,
}

struct FrameUniform {
    viewport: vec2<f32>,
    // Pad to vec4 alignment so the WGSL/std140 layout matches what
    // the Rust side will write via wgpu::util::DeviceExt::create_buffer_init.
    _pad: vec2<f32>,
}

@group(0) @binding(0) var<uniform> frame: FrameUniform;
@group(0) @binding(1) var atlas_tex: texture_2d_array<f32>;
@group(0) @binding(2) var atlas_smp: sampler;

// Map (vertex_index in 0..4) → quad corner in [0,1]². TriangleStrip
// order: (0,0) → (1,0) → (0,1) → (1,1). The bit-twiddle avoids a
// const lookup table and stays within WGSL's small-instruction set.
fn corner_for(vid: u32) -> vec2<f32> {
    return vec2<f32>(
        f32(vid & 1u),
        f32((vid >> 1u) & 1u),
    );
}

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    instance: InstanceIn,
) -> VertexOut {
    let corner = corner_for(vid);

    // Pixel position of this corner.
    let pixel_pos = instance.cell_xy + corner * instance.cell_size;

    // Pixel → NDC. Top-left origin (y flipped).
    let clip_xy = vec2<f32>(
        (pixel_pos.x / frame.viewport.x) * 2.0 - 1.0,
        1.0 - (pixel_pos.y / frame.viewport.y) * 2.0,
    );

    // Linearly interpolate the atlas UV across the quad.
    let u = mix(instance.atlas_uv.x, instance.atlas_uv.z, corner.x);
    let v = mix(instance.atlas_uv.y, instance.atlas_uv.w, corner.y);

    var out: VertexOut;
    out.clip = vec4<f32>(clip_xy, 0.0, 1.0);
    out.uv = vec2<f32>(u, v);
    out.atlas_layer = f32(instance.atlas_layer);
    out.fg = instance.fg_rgba;
    out.bg = instance.bg_rgba;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Sample the atlas at level 0 (no mipmaps; cell glyphs are 1:1).
    let glyph = textureSampleLevel(
        atlas_tex,
        atlas_smp,
        in.uv,
        i32(in.atlas_layer),
        0.0,
    );

    // Alpha is glyph coverage in both rasterization modes.
    let coverage = glyph.a;

    // Per-pixel detection of color-emoji output. Monochrome glyphs
    // are painted in pure white (#ffffff) — getImageData returns
    // non-premultiplied bytes so RGB stays (1,1,1) regardless of
    // alpha. Color-emoji fonts (COLR / CPAL / sbix / SVG) ignore
    // fillStyle and write their native palette into RGB — those
    // pixels have at least one channel below 1.0. The 0.99 threshold
    // tolerates sRGB→linear quantization at boundary white pixels
    // (decoded ~0.992) so monochrome AA edges aren't misclassified
    // as colored.
    let is_color = (glyph.r < 0.99) || (glyph.g < 0.99) || (glyph.b < 0.99);
    let glyph_rgb = select(in.fg.rgb, glyph.rgb, is_color);

    // Composite glyph RGB over bg weighted by coverage. RGB linearly
    // interpolates; alpha goes to 1.0 wherever the glyph paints
    // anything (cells should always be opaque since the renderer's
    // theme.bg already has alpha=1).
    let rgb = mix(in.bg.rgb, glyph_rgb, coverage);
    let a = mix(in.bg.a, 1.0, coverage);
    return vec4<f32>(rgb, a);
}
