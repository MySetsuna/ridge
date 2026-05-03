# ridge-term — Rust terminal kernel (round 1: skeleton)

This is **round 1 of N** in replacing xterm.js with a Rust+WASM terminal
emulator. **It cannot yet replace xterm in your `Pane.svelte`.** What
landed this round is the foundation:

```
┌─────────────────────────────────────────────────────────────────┐
│  JS:  new Terminal(rows, cols, scrollback)                      │
│       term.feed(uint8Array)   ← bytes from PTY                  │
│       term.dumpVisibleText()  ← string[] for smoke testing      │
└──────────────────────┬──────────────────────────────────────────┘
                       │  wasm-bindgen
┌──────────────────────▼──────────────────────────────────────────┐
│  Terminal facade (terminal.rs)                                  │
│  ┌────────────────────────────────────────────────────────┐     │
│  │  vte::Parser  ─────►  Performer (parser.rs)            │     │
│  │  state machine        translates callbacks to grid ops │     │
│  └────────────────────────────────────────────────────────┘     │
│  ┌────────────────────────────────────────────────────────┐     │
│  │  Grid (grid.rs)                                        │     │
│  │  · Vec<Row> visible rows  · cursor + pending_wrap      │     │
│  │  · AttrTable flyweight    · saved_cursor (DECSC)       │     │
│  └────────────────────────────────────────────────────────┘     │
│  ┌────────────────────────────────────────────────────────┐     │
│  │  Scrollback ring buffer (scrollback.rs)                │     │
│  │  · fixed capacity, recycles row allocations on eviction│     │
│  └────────────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────────┘
```

## What works

- VT/ANSI parser via `vte` (full Paul Williams state machine — same as Alacritty)
- C0 controls: BS / HT / LF / VT / FF / CR
- CSI: cursor motion (A/B/C/D/H/f), erase (J/K), scroll (S/T), SGR (m)
- ESC: DECSC / DECRC / IND / NEL / RI
- SGR: 0/1/2/3/4/5/7/8/9/21–29 + ANSI 16 + 256 + truecolor (semicolon and colon forms)
- DECAWM pending-wrap (vim's bottom-right cell renders correctly)
- Wide cells (CJK / emoji) — coarse table, full one next round
- Scrollback with allocation recycling
- 9 unit tests covering the above (`cargo test --lib`)

## Explicitly NOT in this round

These are deliberate omissions, not oversights:

| Feature | Round |
|---|---|
| Renderer (WebGPU/Canvas/DOM) | 2 |
| `onData(cb)` keyboard input | 2 |
| Selection / copy / search | 3 |
| Alt screen buffer | 2 |
| OSC titles / hyperlinks (8) / cwd (7) | 3 |
| DECSTBM scroll regions | 2 |
| DEC private modes (mouse, bracketed paste, cursor visibility) | 3 |
| Resize reflow of soft-wrapped lines | 4 |
| IME helper-textarea integration | 4 |
| `WebLinksAddon` / `SearchAddon` equivalents | 3 |
| Full Unicode-11 wcwidth + emoji-wide override | 2 |

## What I need from you for round 2

Confirm these so I can plan the right surface:

1. **Renderer target:** WebGPU primary + Canvas2D fallback (per your original
   prompt), or just Canvas2D first to ship faster?
2. **PTY transport:** the existing `invoke('write_to_pty', { paneId, data })`
   stays, right? The kernel just needs an `onData(cb)` event — it doesn't
   own the channel.
3. **Scrollback replay:** your current code calls `get_pane_scrollback_tail`
   and `term.write(bytes)`. Same flow works here — `feed()` is the new `write`.

## Build

```bash
cargo test --lib            # native; no wasm needed
wasm-pack build --target web --out-dir pkg --release
```

Sandbox here doesn't have `cargo`, so I haven't run the tests myself — see
"caveat" in the chat reply. If anything fails to compile when you try it,
paste the error and I'll fix in the next round.
