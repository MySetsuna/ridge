---
id: L4-EMOJI-GEOMETRY-001
level: L4
title: Verify normalized emoji geometry
status: LOCKED
parent: L3-TERMINAL-GLYPH-001
code_targets:
  - packages/ridge-term/src/render/glyph_atlas.rs
  - packages/ridge-term/src/render/webgpu.rs
test_targets:
  - packages/ridge-term/src/render/glyph_atlas.rs
  - packages/ridge-term/src/term/wcwidth.rs
---

# Verify normalized emoji geometry

Deterministic tests cover color bitmaps smaller than, larger than, and unequal
to the em box. Every case keeps complete UVs, preserves aspect ratio, centers
inside the reserved cells, and leaves monochrome geometry unchanged. A running
WebGPU terminal fixture confirms decorated and ZWJ emoji are not clipped.
