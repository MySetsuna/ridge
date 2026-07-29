# Iteration 69 Contract — terminal rendering evidence and history geometry

- Date: 2026-07-29
- Status: approved / implementation
- Requirements:
  - `REQ-TERMINAL-RASTER-01`
  - `REQ-CODEX-RENDER-STABILITY-01`
  - `REQ-HISTORY-OVERLAY-GEOMETRY-01`
  - `REQ-AUTO-CONTRAST-RESEARCH-01`

## Delivery boundary

1. History overlay
   - The visible terminal grid, expressed as pane-local rows and columns, is
     the sole clipping rectangle.
   - Shared pure geometry prefers the requested cursor side, flips when the
     opposite side fits, then reduces rows and clamps.
   - Width is capped to the pane; a clipped popup is horizontally centered.
   - Canvas2D and WebGPU consume the same geometry and text-width rules.
   - DPR `1/1.25/1.5/2`, bottom-right, fallback, narrow, and reduced-row
     fixtures assert containment and centering.

2. Raster audit
   - Canvas2D and WebGPU already share `procedural_box`; both align cell
     boundaries before painting.
   - No font/hinting/atlas behavior changes without paired native
     PowerShell captures at the approved DPR/zoom matrix.

3. Codex render audit
   - Existing render order drains feeds before dirty inspection, emits at
     most one focused cursor, and presents one host frame after dirty/cached
     pane composition.
   - No blink, refresh-rate, or renderer heuristic changes without the
     approved Codex/Claude PTY recording and frame trace.

4. Automatic contrast research
   - Compare static token lint, WCAG 2.2, experimental WCAG 3 work,
     forced-colors, and composited-background sampling.
   - Research only. No global runtime recoloring.

## Non-goals

- No host Ridge launch, termination, process-tree intervention, or visual
  claim based on an installed build.
- No CSS blur/transform, global blink disable, refresh throttling, or
  screenshot post-processing.
- No inferred fix where the required native/PTY recording is absent.

## Deterministic gates

- `ridge-term` history geometry unit tests.
- `ridge-term` wasm32 compile, covering both render backends.
- Focused terminal history Vitest and touched Svelte diagnostics.
- `git diff --check`.

## User-track gates

- Native PowerShell comparison at DPR `1/1.25/1.5/2` and zoom
  `100%/125%/150%`, captured once per backend.
- One Codex and one Claude PTY recording replayed with frame-generation and
  cursor trace.

