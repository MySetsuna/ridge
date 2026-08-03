# Iteration 132 — desktop terminal performance and link hover

Date: 2026-08-03

## Outcome

- Desktop terminal rendering now keeps the main-thread shared WebGPU path as
  the default. The legacy OffscreenCanvas worker is opt-in because its current
  `newFromOffscreen` constructor is Canvas2D-only. Pane attach also claims the
  marked shared host canvas before constructing the first renderer, closing the
  mount-order race that permanently selected Canvas2D.
- The extra desktop vertical caret was removed by making the hidden IME
  textarea caret transparent; the WASM renderer remains the sole terminal
  caret owner.
- Startup no longer loses the first `ridge:pane-attached` event. Saved-workspace
  metadata refresh runs off the first-paint path, and inactive keep-alive
  workspaces no longer start Git/SCM polling. Theme image decode is URL
  single-flight/latest-wins; hidden-pane theme invalidation is spread across
  turns.
- Plain-text links now carry one logical identity across soft-wrapped rows.
  Ctrl/Cmd hover paints a continuous 1px underline for every visual segment;
  bare hover shows `按 Ctrl 可跳转` without making a click navigate. The full
  wrapped target remains newline-free for copy/open resolution.
- The same batch includes the already-approved Remote RPC coalescing,
  cached Git/PWA safe-area behavior, and the Kernel bounded PTY HTTP output
  lease. Lease handles are capped at 1024 in addition to the bounded replay
  window; destroy removes outstanding leases.

## Evidence

- `pnpm exec vitest run --reporter=dot`: 146 files, 1510 passed, 1 skipped.
- `pnpm check`: 0 errors / 0 warnings.
- `cargo test -p ridge-term --lib --quiet`: 398 passed.
- `cargo test -p ridge-kernel --lib --quiet`: 30 passed.
- `cargo check -p ridge-kernel --bin ridge-kernel --quiet`: passed.
- Focused link/renderer/SCM tests: 32 passed; `git diff --check`: clean.
- Pushed commits: `2079685`, `c9bc7c0`, `26b4f42`, `afbfa11`, `aeb5d2a` to
  `origin/main`.

## Boundaries and follow-up

This iteration does not claim physical WebView2/GPU-adapter success, a
long-running WebView2 memory soak, mobile notch hardware evidence, public
Remote/Cloud deployment, or a release artifact. No version bump or release was
made because `v0.1.54` consumed today's publication allowance. Browser control
was unavailable in this environment, so those checks remain external gates.
