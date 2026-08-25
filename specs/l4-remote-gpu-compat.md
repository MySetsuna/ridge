---
id: L4-REMOTE-GPU-COMPAT-001
level: L4
title: Keep Remote terminals available across browser GPU backends
status: LOCKED
parent: L3-REMOTE-SMOOTHNESS-001
depends_on:
  - L4-REMOTE-PERFORMANCE-GATE-001
artifact: PSEUDOCODE
code_targets:
  - packages/ridge-term/Cargo.toml
  - packages/ridge-term/README.md
  - packages/ridge-term/build.mjs
  - packages/ridge-term/src/lib.rs
  - packages/ridge-term/src/render/gpu_context.rs
  - packages/ridge-term/src/render/surface_host.rs
  - packages/ridge-term/src/render/webgpu.rs
  - packages/remote/src/shared/terminal/manager.ts
  - src/remote/MainApp.svelte
  - src/remote/lib/TerminalCanvas.svelte
  - scripts/cdp-term-render-e2e.mjs
  - scripts/mobile-keyboard-e2e.mjs
test_targets:
  - packages/remote/src/shared/terminal/manager.attach.test.ts
  - src/remote/lib/TerminalCanvas.test.ts
  - scripts/cdp-term-render-e2e.mjs
  - scripts/mobile-keyboard-e2e.mjs
---

# Keep Remote terminals available across browser GPU backends

```text
create the wgpu instance with WebGPU and WebGL2 compiled in
if a real WebGPU adapter is available: use WebGPU
otherwise: request the wgpu WebGL2 adapter against the host canvas surface
select a linear canvas format advertised by that surface
run the same renderer, glyph atlas, compositor, IME, and input APIs
report the actual backend as WebGPU or WebGL2
if neither backend is available: surface one structured initialization error
never use Canvas2D as a terminal presentation backend or download terminal fonts
```
