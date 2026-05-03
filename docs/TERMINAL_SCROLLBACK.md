# Terminal Scrollback — Current State & Virtual-Scroll Plan

> **Status (2026-05-03):** the "Baseline" section below is the **historical**
> as-of-round-16 (2026-04-25) snapshot that justified the redesign. Phases
> 1–3 of the plan have shipped (see "Phase-plan" table) and xterm was
> retired in round 7. For the current scrollback architecture see:
>
> - **Backend block model:** `src-tauri/src/state.rs::PaneScrollback`
>   (64 KiB blocks, 4 MiB cap, monotonic `seq`).
> - **IPC commands:** `get_pane_scrollback_tail` (newest), `get_pane_scrollback_before`
>   (load older). The legacy `get_pane_scrollback` shim was removed
>   post-round-7.
> - **Frontend replay:** `RidgePane.svelte` mount-time tail replay +
>   `manager.prependScrollback` for `Shift+PageUp` paging past the wasm
>   kernel boundary (TASKS §2.1).
>
> Below text is preserved verbatim for design-history continuity.

Last reviewed: round 16 (2026-04-25).

This document captures what the PTY / xterm scrollback pipeline looks like
today and the design we've agreed for incrementally moving it to virtual,
block-loaded scrolling. Read this first before changing anything in
`state.rs::append_pty_scrollback`, `commands/terminal.rs::get_pane_scrollback`,
or `Pane.svelte`'s xterm initialisation — subtle interactions with resize /
IME / cursor positioning bite hard.

---

## Baseline (what runs today)

### Data path

```
PTY bytes ──► tauri event `pty-output-{ws}-{pane}` ──► xterm.write()  (live)
          └─► AppState.append_pty_scrollback()                        (persistence)
```

- Live output: each PTY chunk is emitted as a `pty-output-*` event and
  consumed by `Pane.svelte`'s listener (`term.write(bytes)`) — xterm owns its
  own viewport + scrollback buffer (`scrollback: 8000` lines).
- Persistence: the same chunk is appended to `AppState.pty_scrollback`
  (`HashMap<(WorkspaceId, PaneId), String>`), a single growing `String`
  per pane capped at **384 KiB** (`MAX` in `state.rs::append_pty_scrollback`).
  On overflow, the oldest bytes are drained; the drain walks forward to the
  next UTF-8 char boundary so we never cut a codepoint in half.

### Retrieval

- `commands/terminal.rs::get_pane_scrollback(pane_id)` reads the string and
  clones it to the caller. Full buffer, one shot.
- `AppState::get_pty_scrollback_tail(ws, pane, max_lines)` exists for line-
  bounded tail reads (not currently wired to the frontend).

### Consumption on reconnect

The frontend doesn't currently replay scrollback when a `Pane.svelte` mounts
— xterm starts empty and fills as new output arrives. Panes that stayed
mounted keep their viewport intact (xterm's own buffer); panes that were
destroyed and re-mounted lose history until somebody wires a replay call.

### Resize behaviour (xterm)

- FitAddon recomputes `cols × rows` on `ResizeObserver` / window resize;
  xterm reflows existing buffer.
- With default `windowsMode: false`, xterm wraps lines to the new column
  count without losing content. `rescaleOverlappingEmoji: true` was tried
  and deprecated in round 2.
- IME composition and cursor positioning edge-cases were hardened in earlier
  rounds (OSC 7 parsing fix in round 9; IME focus guard).

### Observed gaps

1. **Capture is lossy at 384 KiB.** Long-running sessions drop their oldest
   output entirely. A `tail -f production.log` can blow the cap in seconds.
2. **`get_pane_scrollback` returns the entire buffer.** Replaying into xterm
   requires a single big `term.write()` → parser backlog + UI pause on the
   order of seconds for a full 384 KiB dump.
3. **Resize doesn't break anything visible today**, but there is no regression
   test; changes to FitAddon behaviour could regress silently.
4. **No mechanism for "scroll up past xterm's in-memory buffer"** — once
   xterm's own 8000-line scrollback fills, older lines are gone for good,
   even though the backend `pty_scrollback` may still have them.

---

## Target design: block scrollback + on-demand hydrate

### Backend data model

Replace the single `String` with:

```rust
pub struct PaneScrollback {
    /// Completed blocks, ring-indexed. Oldest at `front`, newest at `back`.
    blocks: VecDeque<Arc<[u8]>>,
    /// Global monotonic sequence; first live byte is `seq_head - total_bytes`.
    /// Each flushed block records its starting seq so the client can scroll
    /// to "sequence S" deterministically.
    block_seqs: VecDeque<u64>,
    /// In-flight buffer still being appended. Flushes to `blocks` when full.
    current: Vec<u8>,
    current_start_seq: u64,
    /// Accumulated byte count across all retained blocks + current. Used for
    /// cap eviction.
    total_bytes: usize,
}
```

- `BLOCK_SIZE = 64 KiB` (tuneable); `MAX_BYTES = 4 MiB` default (user-configurable).
- `append` pushes into `current`; when `current.len() == BLOCK_SIZE`, freeze
  it into `blocks` and start a new one. Evict the front when total exceeds
  `MAX_BYTES`.
- Retain UTF-8 safety: freezing a block walks forward to the next codepoint
  boundary, exactly like today.

### New commands

```rust
#[tauri::command]
pub fn get_pane_scrollback_tail(
    state: State<'_, AppState>,
    pane_id: String,
    max_bytes: usize,
) -> Result<ScrollbackChunk, String>;

#[tauri::command]
pub fn get_pane_scrollback_before(
    state: State<'_, AppState>,
    pane_id: String,
    before_seq: u64,
    max_bytes: usize,
) -> Result<ScrollbackChunk, String>;

pub struct ScrollbackChunk {
    pub bytes: String,
    /// Start sequence of the returned bytes; caller uses this as the next
    /// `before_seq` when paging further up.
    pub start_seq: u64,
    /// `true` when we returned the very first (oldest retained) block.
    pub at_oldest: bool,
}
```

Keep `get_pane_scrollback` (deprecated) backed by `get_pane_scrollback_tail(..., usize::MAX)`
for one release so the existing frontend callers don't break while the new
paged path is wired.

### Frontend consumption

In `Pane.svelte` initialisation:

1. On mount, call `get_pane_scrollback_tail(pane_id, 128 KiB)` → `term.write()`.
2. Remember `start_seq`. Hook `term.onScroll`: when `term.buffer.active.viewportY
   <= 16` and not already fetching, call `get_pane_scrollback_before(pane_id,
   start_seq, 64 KiB)`. Prepend into xterm via an offscreen buffer copy
   (xterm's API allows `term.write()` but not "prepend"; we keep a
   shadow `xterm.addons.canvas` or simpler: a SvelteTerminal wrapper that
   maintains a virtual list of blocks and instantiates xterm only for the
   visible window — this is the "virtual scroll" part).
3. At `at_oldest=true`, stop requesting.

The virtual-scroll wrapper (phase 2) is the bigger lift; phase 1 delivers
lossless capture + lazy tail replay with no UI rewrite. Phase 2 can be done
later without breaking the command surface.

### Resize guarantee

Because blocks are raw PTY bytes (not parsed lines), resizing xterm doesn't
have to touch the backend store at all — xterm reflows its in-memory buffer
and our lazy-load path continues to pull older bytes that are still byte-
for-byte what the shell emitted.

QA checklist (manual for now; cargo/playwright doesn't exercise PTY):

- Long `cat large-file` into the terminal, then resize the window wider ⇒
  no line truncation, all bytes visible by scrolling.
- `printf '\033[31mhello\033[0m\n'` before resize ⇒ color still red.
- CJK content visible before resize ⇒ still visible, no double-width artefacts.
- IME composition open during resize ⇒ composition stays visible and commits
  cleanly.

---

## Phase-plan (what to do when)

| Phase | Contents | Rounds | Status |
|---|---|---|---|
| 0 | This doc (baseline + design) | 16 | ✅ |
| 1 | Backend block model + two new `get_*_tail` / `get_*_before` commands; deprecated `get_pane_scrollback` shim kept during transition | 17 | ✅ |
| 2 | Frontend mount-time replay on tail bytes | 17 | ✅ (now in `RidgePane.svelte` after xterm retirement) |
| 3 | Frontend `onScroll` → prepend older blocks; virtual-scroll wrapper | 18 | ✅ shipped via `manager.prependScrollback` + wasm kernel `prepend_scrollback` (TASKS §2.1); no virtual-scroll wrapper needed once xterm was retired |
| 4 | Cap bump from 384 KiB → 4 MiB default; user-configurable `MAX_BYTES` | after phase 2 observed | ✅ (4 MiB default in `state::SCROLLBACK_MAX_BYTES`; user-configurable knob still ⏳ if requested) |

Phases 1–3 shipped and the deprecated `get_pane_scrollback` shim was
removed post-round-7 (xterm retired, paged reads are the only path).

---

## Non-goals (for now)

- Search inside scrollback: xterm has its own search addon; a fuller
  "persistent grep over pty history" is not in scope.
- Exporting scrollback to a file: trivial wrapper over
  `get_pane_scrollback_before` loop when needed.
- ANSI parsing on the backend: we stay pass-through bytes. Any "strip ANSI"
  layer can be a frontend concern.
