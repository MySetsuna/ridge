# ridge-term — Rust terminal kernel + WebGPU renderer (WASM)

`ridge-term` is Ridge's VT/ANSI terminal kernel and WebGPU presentation
backend. It builds the `@ridge/term-wasm` package with wasm-pack.

## Architecture

```text
PTY bytes -> TerminalKernel (VT parser, primary/alternate grid, DECSTBM)
                         |
                         v
          RenderHandle.newWithWebgpuFirst(canvas, SurfaceHostHandle)
                         |
                         v
              Renderer<WebGpuPaneBackend> -> browser GPU surface
```

There is one wgpu presentation pipeline. It prefers browser WebGPU and uses
wgpu's WebGL2 backend when WebGPU is unavailable. Adapter, device, or surface
initialization failure returns an explicit `WEBGPU_INIT_FAILED` error only
when neither backend can render. No Canvas2D presentation or software fallback
is provided.

The glyph atlas may use a hidden browser 2D canvas solely as the system-font
rasterization input required to produce WebGPU textures. That canvas never
presents terminal pixels and is not a renderer backend.

## Kernel coverage

- VT/ANSI parser via `vte`, including CSI/ESC/OSC controls and DSR replies.
- Primary and alternate screens, DEC origin/modes, synchronized output,
  mouse reporting, selection, hyperlinks, IME state, and cursor modes.
- Scrollback, reflow-aware wide characters, and DECSTBM scrolling regions.
- WebGPU glyph atlas, dirty-row rendering, selection/cursor overlays, and
  scroll-copy for normal shell, inline TUI, alternate screen, and DECSTBM.

## Build

```bash
node build.mjs           # release WebGPU-first wasm-pack build
node build.mjs --dev     # development WebGPU-first wasm-pack build
```

`build.mjs` invokes wasm-pack, patches the package name to
`@ridge/term-wasm`, and removes the generated `.gitignore` because `pkg/` is
consumed through the workspace link. `--no-webgpu` is rejected explicitly.

Use the package from the Ridge frontend through `TerminalManager`. The
manager attaches a shared `SurfaceHostHandle`, constructs GPU `RenderHandle`
instances, and surfaces initialization errors to diagnostics.

## Tests

```bash
cargo test --lib
cargo test --tests
```

Rust renderer tests cover scroll-copy invariants for shell, inline TUI,
alternate-screen, and partial DECSTBM regions. TypeScript tests cover the
manager's WebGPU/WebGL2 initialization and worker kernel protocol.
