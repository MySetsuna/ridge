---
id: L3-TERMINAL-GLYPH-001
level: L3
title: Stable system glyph rasterization
status: LOCKED
parent: L2-RUNTIME-QUALITY-001
code_targets:
  - packages/ridge-term/src/render/glyph_atlas.rs
  - packages/ridge-term/src/render/glyph_rasterizer.rs
  - packages/ridge-term/src/render/gpu_context.rs
  - packages/ridge-term/src/render/shaders/cell.wgsl
  - packages/ridge-term/src/render/webgpu.rs
  - packages/ridge-term/src/term/wcwidth.rs
  - src-tauri/src/commands/terminal_font.rs
test_targets:
  - packages/ridge-term/src/render/glyph_atlas.rs
  - packages/ridge-term/src/render/glyph_rasterizer.rs
  - packages/ridge-term/src/render/renderer.rs
  - packages/ridge-term/src/term/wcwidth.rs
  - scripts/cdp-cell-graphics-e2e.mjs
  - scripts/cdp-term-render-e2e.mjs
---

# Stable system glyph rasterization

Terminal grapheme clusters retain parser cell widths while the shared WebGPU
renderer presents complete system-font bitmaps with stable row alignment.
Monochrome glyphs preserve native geometry. Color emoji preserve aspect ratio,
use one em-sized maximum extent, and remain centered without clipping. Local,
Remote, and Cloud surfaces share this contract without external font installs.
Box-drawing glyphs remain selected-font rasters; connector coverage extends to
the Unicode-declared cell sides so adjacent cells meet while curves and source
antialiasing remain intact. No connector repair may translate or rescale the
complete glyph. Each connected side repeats only that side's own native edge
profile through an unpainted bearing; opposite edges never share a profile.
Thus asymmetric Block Elements retain their selected-font silhouette while
rounded corners retain their native curve and tangent axes. Their quads preserve
a one-source-texel-to-one-device-pixel mapping and clip to snapped cell bounds.
Box Drawing and Block Elements are terminal cell graphics: each is presented as
one atlas quad, never a second edge quad that can create a rasterization or blend
boundary.
Color glyphs use the atlas' sole filtering sampler; monochrome text, Box Drawing,
and Block Elements use exact texel loads. This keeps WebGL2 fallback shader
validation legal without softening the WebGPU path. TUI mode obeys DECTCEM and
keeps an enabled cursor steady without blink wakeups. Repaint transactions keep
the last presented cursor through explicit synchronized-output boundaries until
the shared 64 ms quiet window expires, preventing split startup chunks from
presenting transient clear-row cursor positions. No path may threshold Swash
coverage or introduce a non-GPU presentation backend.
