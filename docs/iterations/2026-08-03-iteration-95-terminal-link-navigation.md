# Iteration 95 — Terminal link navigation and wrapped-copy correctness

Date: 2026-08-03  
Status: closed by v0.1.49 publication

## Scope

Fix the installed/Desktop terminal link path for three reported behaviours:

1. Selecting a URL across a visual soft-wrap must copy one logical line, not a
   visual-wrap newline.
2. Ctrl/Cmd-click must open the complete URL/path, not the first row fragment.
3. Holding Ctrl/Cmd while hovering a link must show a visible underline in
   WebView2/Tauri as well as in the shared Remote terminal.

## Root causes

- The Rust selection model already knows the authoritative `Row::wrapped` bit,
  but the JS plain-text link scanner only saw trimmed per-row strings. A URL
  ending at a visual row boundary was therefore indexed as a truncated span.
- Hover state was written only to `data-link-underline*` attributes. No CSS or
  renderer consumed those attributes, so the decision/cursor existed but no
  underline was painted.

## Implementation

- `packages/ridge-term/src/lib.rs` exposes `TerminalKernel.rowWrapped(row)`;
  the generated WASM is rebuilt by `packages/ridge-term/build.mjs`.
- `packages/remote/src/shared/terminal/linkSpans.ts` joins URL/file/path
  spans only across authoritative soft-wrap rows (with a conservative full-
  row fallback for older bundles), preserves per-row hit regions, and carries
  the complete target text to Ctrl/Cmd-click.
- `packages/remote/src/shared/terminal/manager.ts` adds a pointer-events-free
  DOM underline overlay, positions it from the same pane geometry as pointer
  hit-testing, expands OSC 8 ranges, and clears it on modifier release,
  pointer leave, TUI mouse mode, scrollback clear, park, and detach.
- `packages/ridge-term/src/selection.rs` adds a URL-specific regression test;
  the existing Rust selection implementation remains the single authority for
  preserving hard newlines while omitting soft-wrap newlines.

## Verification

- `pnpm exec vitest run packages/remote/src/shared/terminal/linkSpans.test.ts packages/remote/src/shared/terminal/linkAffordance.test.ts packages/remote/src/shared/terminal/linkOpenHost.test.ts packages/remote/src/shared/terminal/mobileCopy.test.ts`: 23/23.
- Full Vitest: 142 files, 1475 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- `cargo test --manifest-path packages/ridge-term/Cargo.toml --lib`: 397/397.
- `node packages/ridge-term/build.mjs`: WASM build completed; wasm-pack and
  wasm-opt emitted only environment warnings (the repository intentionally
  ignores generated `packages/ridge-term/pkg`).

## Commit / release

Runtime commit: `06f5f74` (`fix(terminal): preserve wrapped links and show hover underline`), pushed to `origin/main`. Version commit `c163ed4` aligned all four version sources to `0.1.49`; annotated tag `v0.1.49` passed the clean release gate and its matrix produced 12 matching assets. Remote/Cloud workflow `30771421397` succeeded from the exact tag and Cloud health returned HTTP 200 (`version=0.0.7`).

## Residual gates

Physical phone/WebView2 visual confirmation, public WebRTC, dual-window Remote
workspace singleton, production branch identity, and full Kernel-authority
migration remain tracked residuals from earlier iterations.
