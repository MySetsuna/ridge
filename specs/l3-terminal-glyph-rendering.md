---
id: L3-TERMINAL-GLYPH-001
level: L3
title: Stable system glyph rasterization
status: LOCKED
parent: L2-RUNTIME-QUALITY-001
code_targets:
  - packages/ridge-term/src/render/glyph_atlas.rs
  - packages/ridge-term/src/render/shaders/cell.wgsl
  - packages/ridge-term/src/render/webgpu.rs
  - packages/ridge-term/src/term/wcwidth.rs
test_targets:
  - packages/ridge-term/src/render/glyph_atlas.rs
  - packages/ridge-term/src/term/wcwidth.rs
  - scripts/cdp-term-render-e2e.mjs
---

# Stable system glyph rasterization

Terminal grapheme clusters retain parser cell widths while the shared WebGPU
renderer presents complete system-font bitmaps with stable row alignment.
Monochrome glyphs preserve native geometry. Color emoji preserve aspect ratio,
use one em-sized maximum extent, and remain centered without clipping. Local,
Remote, and Cloud surfaces share this contract without external font installs.
The shared GPU compositor sharpens monochrome antialiasing coverage without
altering color emoji alpha or introducing a non-GPU presentation path.
At DPR 1 the native glyph fixture requires at least 70% solid monochrome
coverage and at most 30% transition coverage; DPR 1.25 remains a visual and
geometry regression gate.
