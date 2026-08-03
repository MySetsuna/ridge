# Iteration 122 - cross-cutting regression after PWA, Host, and terminal-link slices

Date: 2026-08-03  
Status: regression complete; release deferred by the daily publication freeze

## Verification

- Full Vitest: 145 files, 1,497 passed, 1 skipped.
- `cargo test -p ridge --lib hosts:: --quiet`: 59 passed.
- `cargo test -p ridge-term --lib --quiet`: 398 passed.
- `pnpm check`: 0 errors, 0 warnings.
- `pnpm build:remote:mobile`: succeeded; PWA manifest/service worker/safe-area
  verifier passed.
- `scripts/rdg-remote-e2e.mjs --skip-build`: LAN desktop/mobile PASS with
  `ws=true`, canvas/input/resize probes, and `browserErrors=[]`.
- `scripts/mobile-keyboard-e2e.mjs`: keyboard convergence/recovery and touch
  selection PASS with `browserErrors=[]` (Chromium emulation, not a physical
  phone).

The PWA notch, Host topology refresh, SCM cadence, and terminal-link fixes are
all pushed on `main`. Physical notch/WebView2, public WebRTC, authenticated Git
push, and long-run heap/dual-window evidence remain external. No version bump or
publication was made because `v0.1.54` consumed today's release allowance.
