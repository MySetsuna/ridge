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
antialiasing remain intact. Rounded corners reuse the same selected-font
straight-line profiles and axes: the complete native curve translates to the
horizontal reference axis before vacated vertical arms are extended, so no
outer-edge splice can detach an interior tangent. Their quads preserve a
one-source-texel-to-one-device-pixel mapping and clip to snapped cell bounds
without geometric rescaling.
Monochrome text and box drawing sample at native nearest density; only scaled
color emoji use linear atlas sampling. TUI mode obeys DECTCEM and keeps an
enabled cursor steady without blink wakeups. No path may threshold Swash
coverage or introduce a non-GPU presentation backend.
