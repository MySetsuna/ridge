---
id: L3-TERMINAL-GLYPH-001
level: L3
title: Stable system glyph rasterization
status: LOCKED
parent: L2-RUNTIME-QUALITY-001
code_targets:
  - packages/ridge-term/src/render/glyph_atlas.rs
  - packages/ridge-term/src/render/webgpu.rs
  - packages/ridge-term/src/term/wcwidth.rs
test_targets:
  - packages/ridge-term/src/render/glyph_atlas.rs
  - packages/ridge-term/src/term/wcwidth.rs
---

# Stable system glyph rasterization

Terminal grapheme clusters retain parser cell widths while the shared WebGPU
renderer presents complete system-font bitmaps with stable row alignment.
Monochrome glyphs preserve native geometry. Color emoji preserve aspect ratio,
use one em-sized maximum extent, and remain centered without clipping. Local,
Remote, and Cloud surfaces share this contract without external font installs.
