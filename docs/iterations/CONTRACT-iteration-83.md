# Iteration 83 Contract — Mobile Remote IME Stability

## Scope

Carry the approved mobile Remote keyboard requirement into a deterministic,
reproducible closure slice. Preserve the existing keyboard and terminal input
architecture; repair only the stale IME-anchor boundary observed after a shell
submission, and make the regression executable in CI/local iteration runs.

## Root cause

Mobile `TerminalCanvas` sends Enter through direct `onStdin` paths. Those paths
did not clear the terminal manager's pre-submit `imeAnchor`, while
`inputAnchorCell` intentionally preferred that marker before the live cursor.
After a command printed a new prompt, the hidden textarea could therefore stay
anchored to the old prompt. VisualViewport resize then translated the terminal
against stale geometry, producing unstable keyboard placement and input delay.

## Implemented

- `TerminalManager.clearInputStart()` now clears both input-start bookkeeping and
  the obsolete pre-submit IME anchor.
- Physical Enter and virtual-keyboard Enter in `TerminalCanvas.svelte` use the
  same boundary reset as desktop input.
- Keyboard-shift application repositions the focused IME sink after each target
  update, so a shell cursor move between the button click and the next frame
  cannot make the browser scroll toward stale geometry.
- Added `scripts/mobile-keyboard-e2e.mjs` and the
  `e2e:rdg-mobile-keyboard` package command. It starts a real LAN Remote host
  and PTY, uses a fresh Chromium mobile context with extensions disabled,
  synthesizes VisualViewport-only IME resize/jitter, and asserts:
  settled negative bounded translation, input-bottom above keyboard-top,
  keyboard-close recovery/focus, and zero browser errors.

## Verification

- `pnpm check`: 0 errors, 0 warnings.
- Targeted IME/terminal tests: 6 files, 65 passed.
- Full Vitest: 120 files, 1,375 passed, 1 skipped.
- `pnpm build:remote:mobile`: exit 0; only existing empty PWA glob warnings.
- `pnpm e2e:rdg-mobile-keyboard`: `ok=true`; visual viewport 844→400 px,
  settled shifts −400…−384 px, every sampled input bottom stayed above the
  keyboard top, restored shift 0, focus retained, browser errors 0.
- `pnpm e2e:rdg-lan -- --skip-build`: desktop and mobile matrix passed;
  canvas/tree/WS checks passed and browser errors were empty.
- Commit `16d2861` pushed to `origin/main`.
- Version contract `0.1.29` (`b58815e`) and archive (`9ece51d`) were pushed;
  Release run `30717760186` completed successfully, the draft was promoted to
  `v0.1.29`, and all 12 platform assets were verified. Remote publish run
  `30717808784` succeeded at head `9ece51d`; ridge-cloud deploy run
  `30717810568` succeeded and passed its production health check.

## Non-claims and follow-up evidence

This browser probe is deterministic emulation, not physical-device evidence.
The affected phone still needs clean-profile/extension A-B attribution for
`runtime.lastError`; WebView2 heap soak, public Remote long-run, and dual-window
/dual-host physical evidence remain external gates. No Console filtering or
Chrome Extension Messaging code was added.

## Closure status

The code slice is complete and pushed. The parent requirements remain active
until the external gates above are recorded.
