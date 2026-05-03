# ridge-term — Rust terminal kernel + Canvas2D renderer (WASM)

In-house VT/ANSI terminal emulator for the Ridge IDE — replaces xterm.js +
WebGL addon. Compiled to a single `@ridge/term-wasm` package via wasm-pack
and consumed by `src/lib/terminal/manager.ts` in the Ridge frontend.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  JS:  new TerminalKernel(rows, cols, scrollback)                │
│       kernel.feed(uint8Array)         ← bytes from PTY          │
│       kernel.encodeKey(...)           → bytes to PTY            │
│       kernel.takePendingResponse()    → DSR/DA bytes            │
│       kernel.takePendingEvents()      → KernelEvent[] (OSC/Bell)│
│       new RenderHandle(canvas)        ← rAF render driver       │
│       handle.render(kernel) / .resize(w, h, dpr) / .applyTheme  │
└──────────────────────┬──────────────────────────────────────────┘
                       │  wasm-bindgen
┌──────────────────────▼──────────────────────────────────────────┐
│  Terminal (term/terminal.rs)                                    │
│   · vte::Parser → Performer → Grid                              │
│   · pending_response (DSR/DA query replies → PTY)               │
│   · pending_events (Title/Cwd/Bell → JS Svelte stores)          │
│   · prepend_scrollback (sandbox feed → push_front; for backend  │
│     scrollback bridge, see TASKS §2.1)                          │
│                                                                 │
│  Grid (term/grid.rs)                                            │
│   · primary + alt screen, DECSTBM region, AttrTable flyweight   │
│   · reflow_primary on column change (Phase 1: live grid;        │
│     wide-char split protected, cursor pending_wrap preserved)   │
│                                                                 │
│  Renderer (render/renderer.rs)                                  │
│   · per-row dirty tracking via content hash                     │
│   · selection overlay anti-stacking (force selection rows in    │
│     dirty_rows when partial redraw)                             │
│   · cursor blink state machine                                  │
│                                                                 │
│  Canvas2dBackend (render/canvas2d.rs)                           │
│   · RenderBackend trait impl; resize_surface keeps CSS at 100%  │
│     so canvas tracks container; HTML width/height = device px   │
│   · selection_bg / hyperlink underline / cursor block-bar-line  │
└─────────────────────────────────────────────────────────────────┘
```

## Status

End-to-end functional. xterm.js is retired in the consumer; Ridge's
terminal panes run on this kernel exclusively. See
`docs/term-rebuild/TASKS.md` and `docs/term-rebuild/OVERVIEW.md` for the
full progress log and remaining deferred items.

| Round | Scope | Status |
|---|---|---|
| 1   | VT kernel skeleton                                  | ✅ |
| 2.1 | wcwidth + alt screen + DECSTBM + DEC modes          | ✅ |
| 2.2 | RenderBackend trait + Canvas2D backend              | ✅ |
| 2.3 | JS surface API (write/onData/resize/key encoder)    | ✅ |
| 2.4 | TerminalManager (TS) + RidgePane.svelte             | ✅ |
| 3   | WebGPU backend + glyph atlas + shared surface       | ⏳ not started |
| 4   | reflow Phase 2 / IME v3 / grapheme                  | ⏳ partial |
| 5   | OSC UI integration                                  | ✅ |
| 6   | parking lot (split survival)                        | ✅ |
| 7   | xterm retirement                                    | ✅ |

## What's implemented

- **VT/ANSI parser** via `vte` (Paul Williams state machine — same as
  Alacritty)
- **C0 controls** (BS / HT / LF / VT / FF / CR / BEL → KernelEvent::Bell)
- **CSI**: cursor motion (A/B/C/D/E/F/G/`/d/H/f), erase (J/K), scroll
  (S/T), insert/delete (L/M, ICH `@`, DCH `P`), erase-char (ECH `X`),
  repeat (REP `b`), HPR/VPR (`a`/`e`), SCO save/restore (`s`/`u`),
  DECSTBM (`r`), mode set/reset (h/l), SGR (m), DSR/DECXCPR (n), DA
  primary/secondary (c, `>c`), cursor shape (DECSCUSR ` q`)
- **ESC**: DECSC / DECRC / IND / NEL / RI / DECPAM / DECPNM / RIS
- **SGR**: 0/1/2/3/4/5/7/8/9/21..29 flags, ANSI 16 (30..37 / 40..47 /
  90..97 / 100..107), 256 (38;5;n / 48;5;n), truecolor (38;2;r;g;b),
  colon-subparam form (38:2:cs:r:g:b)
- **Screen modes**: primary + alt (?47 / ?1049), DECAWM pending-wrap,
  DECTCEM, cursor blink (?12), application keypad (?1), bracketed paste
  (?2004), synchronous output (?2026), focus reporting (?1004), mouse
  (?9 / ?1000 / ?1002 / ?1003 / ?1006), DEC origin (?6), insert (4),
  LNM (20)
- **OSC**: 0/1/2 title (→ KernelEvent::TitleChanged /
  IconNameChanged), 7 cwd (→ CwdChanged), 8 hyperlinks (cell-level
  annotation; Ctrl+click via `kernel.hyperlinkAt(row, col)`)
- **Wide chars**: CJK + emoji wcwidth=2; reflow-aware split protection
- **Scrollback**: fixed-capacity ring with allocation recycling +
  `prepend_scrollback` for the backend scrollback bridge

## Tests

```bash
cargo test --lib                     # 113 unit tests
cargo test --tests                   # 22 integration tests
```

Integration tests (`tests/protocol_smoke.rs`) cover realistic byte-stream
scenarios: DSR-CPR, PSReadLine prompt redraw, Ink frame replace, ECH
char-residue repro, ?1049 alt-screen round-trip, OSC 8 cross-feed
persistence, ICH+DCH inline edit, `?2026` toggle, `?1004` focus
reporting, REP, RIS, OSC 133/633 prompt marks (no render), and OSC
events in order.

## Build

```bash
node build.mjs           # release build (wasm-pack --release + wasm-opt -Oz)
node build.mjs --dev     # dev build (~5× faster compile, larger wasm)
```

`build.mjs` runs wasm-pack with `--target web`, patches
`pkg/package.json` to the scoped name `@ridge/term-wasm`, and removes
the auto-generated `pkg/.gitignore` so the consumer's
`link:packages/ridge-term/pkg` works on fresh clones.

## Consumer

The Ridge frontend imports the published surface as `@ridge/term-wasm`
(linked locally via `link:packages/ridge-term/pkg`). Entry points:

- `src/lib/terminal/manager.ts` — single-instance `TerminalManager`
  owns the wasm kernels + RenderHandles, runs the rAF loop, exposes
  `feed`/`onData`/`onResize`/`attach`/`park`/`unpark`/`detach`/
  `prependScrollback`/etc.
- `src/lib/terminal/ptyBridge.ts` — host-aware sidecar; subscribes to
  Tauri `pty-output-{ws}-{pane}` events and the `pane-pty-closed`
  rebuild path. Listener lifetime tracks kernel lifetime, surviving
  Svelte component mount cycles (so split / reparent doesn't drop bytes).
- `src/lib/terminal/themeBridge.ts` — pushes Ridge's CSS-variable theme
  into the wasm Theme overrides on settings change.
- `src/lib/components/RidgePane.svelte` — Svelte component that mounts
  the canvas and wires keyboard / mouse / IME events.
