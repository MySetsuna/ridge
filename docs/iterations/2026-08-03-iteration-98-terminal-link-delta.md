# Iteration 98 — desktop terminal link fidelity

Date: 2026-08-03  
Status: code closed; release gate pending

## User-visible defects

- Copying a selection split by terminal soft-wrap inserted a newline.
- Ctrl+click opened only the first visual row of a wrapped URL.
- Ctrl-hover did not reliably show the underline when the pointer was already
  stationary over the link before the modifier was pressed.

## Root cause

The desktop installation enables the Rust parser `delta_mode`. Its delta
protocol transferred cells but not the row-level `wrapped` bit. The native
parser therefore knew the logical-line boundary while the WASM mirror did not;
the mirror's selection and link scanner treated a visual wrap as a hard line
break. The hover path only recalculated on pointer movement, so modifier
changes alone left stale affordance state.

## Implementation

- `packages/ridge-term/src/term/delta.rs`
  - protocol version `3 -> 4`;
  - `GridDelta::Cells` now carries `wrapped`, including wrap-only updates;
  - `ScrollbackAppend` uses `DeltaLine { cells, wrapped }`.
- `src-tauri/src/engine/parser.rs`
  - tracks per-row wrap metadata beside the cell snapshot;
  - emits wrap-only deltas without retransmitting a full row;
  - carries wrap bits for new scrollback rows.
- `packages/ridge-term/src/term/grid.rs` and `term/terminal.rs`
  - apply live and scrollback row metadata to the mirror.
- `packages/remote/src/shared/terminal/manager.ts`
  - re-runs Ctrl/Meta hover hit-testing on modifier keydown/keyup;
  - retains only pointer geometry, not DOM events, and unregisters listeners
    on detach/park.

## Verification

- `cargo test -p ridge-term --lib --quiet`: 397 passed.
- `cargo test -p ridge engine::parser --lib --quiet`: 23 passed.
- `pnpm check`: 0 errors, 0 warnings.
- Full Vitest: 142 files, 1475 passed, 1 skipped.
- New parser-to-mirror regression proves a wrapped URL copies as one logical
  line after a delta frame.

## Release gate

The code must be committed and pushed, then published as a versioned release
with the normal 12 desktop assets. Remote and cloud publication remain
separate checks. Physical WebView2 install verification should confirm the
three reported interactions against the new artifact.
